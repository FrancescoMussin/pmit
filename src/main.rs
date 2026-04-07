#![allow(warnings)]

use anyhow::{Context, Result};
use dialoguer::{theme::ColorfulTheme, Select};
use lru::LruCache;
use pmit::config::Config;
use pmit::data_structures::WalletAddress;
use pmit::database_handler::{
    shutdown_database_cleanly, TradeDatabaseHandler,
    UserHistoryDatabaseHandler,
};
use pmit::ingestor::TradeIngestor;
use pmit::investigator::{InvestigationRequest, MarketDistributions, UserActivityProfiler, spawn_investigation};
use pmit::polymarket::gamma_api::PolymarketGammaApi;
use pmit::polymarket::{self, Side, Trade};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

use futures_util::stream::{StreamExt, FuturesUnordered};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    /// we start formatting the tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();
    
    // we load the env files into a config struct.
    let config = Config::load()?;
    // we initialize the database handlers
    let trades_db = TradeDatabaseHandler::new(config.trades_db_path.clone()).await?;
    trades_db.init_schema().await?;
    let user_history_db =
        UserHistoryDatabaseHandler::new(config.user_history_db_path.clone()).await?;
    user_history_db.init_schema().await?;

    tracing::info!("Loaded config!");
    tracing::info!("Trades DB initialized at {}", config.trades_db_path);

    // initialize caches.
    let mut recent_users: LruCache<WalletAddress, ()> = LruCache::new(
        NonZeroUsize::new(1000).context("Invalid nonzero size for recent users LRU cache")?,
    );
    let mut processed_trades: LruCache<String, ()> = LruCache::new(
        NonZeroUsize::new(5000).context("Invalid nonzero size for processed trades LRU cache")?,
    );

    // initialize the reqwest client and active struct
    let http_client = reqwest::Client::new();
    let trade_ingestor = TradeIngestor::new();
    let user_activity_profiler = UserActivityProfiler::new();

    // =============== GAMMA API EVENT SELECTION ===============
    let gamma_api = PolymarketGammaApi::new(http_client.clone(), config.polymarket_gamma_api_url.clone());
    tracing::info!("Fetching active events from Gamma API...");
    let events = gamma_api.fetch_active_events(200).await?;

    if events.is_empty() {
        tracing::warn!("No active events found!");
        return Ok(());
    }
    // we add the list of events for selection
    let event_titles: Vec<String> = events.iter().map(|e| e.title.clone()).collect();

    // we let the user select a single event they want to monitor
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select an event to monitor (Enter to confirm)")
        .items(&event_titles)
        .default(0)
        .interact()
        .context("Failed to read user selection")?;

    let selected_event = &events[selection];
    tracing::info!("Monitoring event: {}", selected_event.title);

    // A mapping from token ID to a tuple of (Question, Outcome)
    let mut monitored_tokens: HashSet<String> = HashSet::new();
    let mut token_to_outcome_info: HashMap<String, (String, String)> = HashMap::new();

    // Map selected event's markets
    for market in &selected_event.markets {
        // we get the token ids for the given market
        let tokens = market.parsed_clob_token_ids();
        let outcomes = market.parsed_outcomes();
        for (i, token_id) in tokens.iter().enumerate() {
            monitored_tokens.insert(token_id.clone());
            let outcome_str = outcomes.get(i).cloned().unwrap_or_else(|| format!("Outcome {}", i));
            token_to_outcome_info.insert(token_id.clone(), (market.question.clone(), outcome_str));
        }
        tracing::info!("Watching Market: {} with {} tokens", market.question, tokens.len());
    }

    // =============== PRE-WATCHDOG PROFILING ===============
    tracing::info!("Initializing distribution profiles from recent global trades...");
    let mut market_distributions = MarketDistributions::new();
    for token_id in &monitored_tokens {
        market_distributions.setup_token(token_id.clone(), config.ewma_alpha);
    }

    // Capture the event ID for fetching all trades in this event
    let event_id = selected_event.id.clone();
    let event_id_for_bg = event_id.clone();

    match polymarket::fetch_event_trades_raw_json(&http_client, &config.polymarket_data_api_url, &event_id, 1000).await {
        Ok(raw_payload) => {
            if let Ok(ingested_batch) = trade_ingestor.ingest_raw_value(raw_payload, &trades_db).await {
                for trade in ingested_batch.into_trades() {
                    if monitored_tokens.contains(trade.asset.as_str()) {
                        market_distributions.insert_historical_trade(trade.asset.as_str(), &trade);
                    }
                }
            }
        }
        Err(e) => tracing::warn!("Failed to fetch initial history: {:?}", e),
    }

    // Print starting distribution data for user visibility
    println!("\n{:<60} | {:<15} | {:<12} | {:<12}", "Market Question", "Outcome", "Avg Value", "Std Dev");
    println!("{}", "=".repeat(105));
    for (token_id, dist) in &market_distributions.distributions {
        let (question, outcome) = token_to_outcome_info.get(token_id).cloned().unwrap_or_else(|| ("Unknown".to_string(), "Unknown".to_string()));
        let avg = dist.ewma_val_mean.unwrap_or(0.0);
        let std = dist.ewma_val_variance.sqrt();
        
        let display_q = if question.len() > 57 { 
            format!("{}...", &question[..57]) 
        } else { 
            question.to_string() 
        };
        let display_o = if outcome.len() > 12 {
            format!("{}...", &outcome[..12])
        } else {
            outcome.to_string()
        };

        println!("{:<60} | {:<15} | ${:<11.2} | ${:<11.2}", display_q, display_o, avg, std);
    }
    println!("");

    tracing::info!("Distribution initialization complete.");

    // =============== PROCESSING PIPELINE CACHE & CHANNELS ===============
    // we initialize the mpsc channel in which we send batches of trades
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<Trade>>(100);
    // channel for investigation requests
    let (inv_tx, mut inv_rx) = tokio::sync::mpsc::unbounded_channel::<InvestigationRequest>();
    
    // we initialize the interval for polling the data api
    let mut poll_interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    // we clone the config, the http client and the user history db to move them into the async task
    let config_clone = config.clone();
    let http_client_clone = http_client.clone();
    let user_history_db_clone = user_history_db.clone();
    let monitored_tokens_clone = monitored_tokens.clone();
    let token_to_outcome_info_clone = token_to_outcome_info.clone();

    // Start the Investigator Task (The Orchestrator)
    let inv_client = http_client.clone();
    let inv_db = user_history_db.clone();
    let inv_api_url = config.polymarket_data_api_url.clone();
    let investigator_handle = tokio::spawn(async move {
        tracing::info!("Investigator task (Orchestrator) started.");
        // we initialize the futures unordered so that we don't block the investigator_handle on a single investigation
        let mut pending_investigations = FuturesUnordered::new();

        loop {
            tokio::select! {
                // We listen for new investigation requests from the monitor
                Some(req) = inv_rx.recv() => {
                    tracing::info!(">>> [ORCHESTRATOR] Received investigation request for address: {}", req.address());
                    // We spawn a new investigation and add it to our tracking bucket
                    pending_investigations.push(spawn_investigation(req, inv_client.clone(), inv_api_url.clone()));
                }
                
                // We listen for any completed investigation from our bucket
                Some(res) = pending_investigations.next() => {
                    match res {
                        Ok(Ok(report)) => {
                            let address = report.address;
                            let p_value = report.p_value;
                            let past_trades = report.past_trades;
                            
                            tracing::info!(
                                ">>> ✅ [ORCHESTRATOR] Investigation complete for {}. p_value: {:.4}. Win Rate: {:.1}% (Sample: {} history).",
                                address,
                                p_value,
                                report.win_rate * 100.0,
                                past_trades.len()
                            );

                            // For now, we just persist the results. Later, we'll add statistics and heuristics.
                            if let Err(e) = inv_db.insert_user_activity_snapshot(address.to_string(), past_trades).await {
                                tracing::error!("❌ [ORCHESTRATOR] PERSIST ERROR for {}: {:?}", address, e);
                            }
                        }
                        Ok(Err(e)) => {
                            tracing::error!("❌ [ORCHESTRATOR] INVESTIGATION FAILED: {:?}", e);
                        }
                        Err(e) => {
                            tracing::error!("❌ [ORCHESTRATOR] TASK PANIC or ABORTED: {:?}", e);
                        }
                    }
                }
                
                // if both streams are closed, we finish
                else => break,
            }
        }
        tracing::info!("Investigator task (Orchestrator) shutting down.");
    });

    // this is the processing handle that's gonna profile the trades pass the proxyWallets down to the investigator
    let inv_tx_bg = inv_tx.clone();
    let processing_handle = tokio::spawn(async move {
        // we move the variables into the async task.
        // since by default variables used in this block
        // are moved as immutable we need to specify mutability.
        let mut processed_trades = processed_trades;
        let mut recent_users = recent_users;
        let mut stale_streak: u32 = 0;
        let mut dists = market_distributions; // Move distributions into the async task
        let monitored_tokens = monitored_tokens_clone; // Move monitored tokens into async task
        let token_to_outcome_info = token_to_outcome_info_clone; // Move info lookup into async task
        let event_id = event_id_for_bg; // Move cloned event ID into async task
        let inv_tx = inv_tx_bg; // Move investigation sender into async task

        tracing::info!("Background processing task started for event: {}.", event_id);

        // this block runs whenever we get a batch of trades down the channel.
        while let Some(trades) = rx.recv().await {
            // in case we get a stale feed
            stale_streak = update_stale_feed_streak(
                &trades,
                stale_streak,
                config_clone.stale_feed_warn_secs,
                config_clone.stale_feed_consecutive_polls,
            );
            // we extract the unseen trades
            let new_trades = extract_unseen_trades(trades, &mut processed_trades);
            // if we've already seen everything that's in there there's not much else to do.
            if new_trades.is_empty() {
                continue;
            }

            tracing::info!(">>> Processed batch! {} unseen global trades extracted.", new_trades.len());

            // Routing -> UserActivityProfiler (History Fetching)
            user_activity_profiler.profile_batch(
                new_trades,
                &monitored_tokens,
                &mut dists,
                &mut recent_users,
                &inv_tx,
                &config_clone,
            );

            // Print updated distribution stats for visibility per batch
            println!("\n[UPDATED MARKET STATISTICS]");
            println!("{:<60} | {:<15} | {:<12} | {:<12}", "Market Question", "Outcome", "Avg Value", "Std Dev");
            println!("{}", "=".repeat(105));
            for (token_id, dist) in &dists.distributions {
                let (question, outcome) = token_to_outcome_info.get(token_id).cloned().unwrap_or_else(|| ("Unknown".to_string(), "Unknown".to_string()));
                let avg = dist.ewma_val_mean.unwrap_or(0.0);
                let std = dist.ewma_val_variance.sqrt();
                let display_q = if question.len() > 57 { format!("{}...", &question[..57]) } else { question.to_string() };
                let display_o = if outcome.len() > 12 { format!("{}...", &outcome[..12]) } else { outcome.to_string() };
                println!("{:<60} | {:<15} | ${:<11.2} | ${:<11.2}", display_q, display_o, avg, std);
            }
            println!("");
        }

        tracing::info!("Background processing task shutting down.");
    });

    tracing::info!(
        "Starting global trade monitor... Polling every {}s",
        config.poll_interval_secs
    );

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Shutdown signal received. Cleaning up...");
                break;
            }
            _ = poll_interval.tick() => {
                tracing::debug!("--> Polling Data API for new trades...");

                // Targeted poll for selected event
                match polymarket::fetch_event_trades_raw_json(
                    &http_client,
                    &config.polymarket_data_api_url,
                    &event_id,
                    config.global_trades_limit,
                )
                .await
                {
                    Ok(raw_payload) => {
                        let ingested_batch = match trade_ingestor.ingest_raw_value(raw_payload, &trades_db).await
                        {
                            Ok(batch) => batch,
                            Err(e) => {
                                tracing::error!("Error ingesting raw trades payload: {:?}", e);
                                continue;
                            }
                        };

                        let trades = ingested_batch.into_trades();
                        if trades.is_empty() {
                            continue;
                        }
                        // we send the trades over the channel
                        if let Err(e) = tx.try_send(trades) {
                            tracing::warn!("Processing channel full or closed! Batch dropped: {:?}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error fetching targeted market trades: {:?}", e);
                    }
                }
            }
        }
    }

    drop(tx);
    drop(inv_tx);
    let _ = tokio::join!(processing_handle, investigator_handle);

    if let Err(e) = trades_db.shutdown_cleanly().await {
        tracing::error!("Failed to clean shutdown trades DB: {:?}", e);
    }
    if let Err(e) = user_history_db.shutdown_cleanly().await {
        tracing::error!("Failed to clean shutdown user history DB: {:?}", e);
    }

    tracing::info!("Shutdown checkpoint complete.");
    Ok(())
}

fn update_stale_feed_streak(
    trades: &[Trade],
    current_streak: u32,
    stale_feed_warn_secs: u64,
    stale_feed_consecutive_polls: u32,
) -> u32 {
    let Some(newest_trade_ts) = trades.iter().map(|t| t.timestamp).max() else {
        return current_streak;
    };

    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let lag_secs = now_ts.saturating_sub(newest_trade_ts);

    if lag_secs >= stale_feed_warn_secs {
        let next_streak = current_streak + 1;
        tracing::warn!(
            "[STALE FEED] newest trade is {}s old (threshold={}s, streak={})",
            lag_secs,
            stale_feed_warn_secs,
            next_streak
        );

        if next_streak >= stale_feed_consecutive_polls {
            tracing::warn!(
                "[STALE FEED] sustained staleness detected. This often indicates CDN/API cache windows."
            );
        }

        next_streak
    } else {
        if current_streak > 0 {
            tracing::info!(
                "Feed freshness recovered after {} stale poll(s). Current lag={}s",
                current_streak,
                lag_secs
            );
        }
        0
    }
}

fn extract_unseen_trades(
    trades: Vec<Trade>,
    processed_trades: &mut LruCache<String, ()>,
) -> Vec<Trade> {
    let mut unseen = Vec::new();

    for trade in trades {
        if !processed_trades.contains(&trade.transaction_hash) {
            processed_trades.put(trade.transaction_hash.clone(), ());
            unseen.push(trade);
        }
    }

    unseen
}
