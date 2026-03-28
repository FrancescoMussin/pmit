"""
Exposure scorer using sentence-BERT embeddings and cosine similarity.
Designed for fast batch scoring of trades with continuous [0,1] exposure scores.
"""

import sqlite3
import numpy as np
from typing import List, Tuple
from sentence_transformers import SentenceTransformer, util


class ExposureScorer:
    def __init__(self, model_name: str = "all-MiniLM-L6-v2"):
        """
        Initialize the scorer with a sentence-BERT model.
        
        Args:
            model_name: HuggingFace model identifier (default is fast and accurate)
        """
        self.model = SentenceTransformer(model_name)
        self.training_embeddings = None
        self.training_scores = None
        self._load_training_data()
    
    def _load_training_data(self, training_db: str = "databases/training.db"):
        """Load and embed all training samples."""
        conn = sqlite3.connect(training_db)
        cursor = conn.cursor()
        cursor.execute("SELECT title, outcome, exposure FROM training_samples")
        rows = cursor.fetchall()
        conn.close()
        
        if not rows:
            raise ValueError("No training samples found in training.db")
        
        titles, outcomes, scores = zip(*rows)
        
        # Combine title and outcome for richer context
        texts = [f"On the market '{title}' I am betting '{outcome}'".strip() for title, outcome in zip(titles, outcomes)]
        
        # Embed all training samples
        self.training_embeddings = self.model.encode(texts, convert_to_tensor=True)
        self.training_scores = np.array(scores, dtype=np.float32)
        
        print(f"Loaded {len(texts)} training samples")
    
    def score_trade(self, title: str, outcome: str) -> float:
        """
        Score a single trade based on title and outcome.
        
        Uses weighted average of cosine similarities to training examples.
        High similarity to high-exposure trades → high score.
        
        Args:
            title: Market title
            outcome: Trade outcome
        
        Returns:
            Exposure score in [0, 1]
        """
        if not title or not outcome:
            return 0.0
        
        text = f"On the market '{title}' I am betting '{outcome}'".strip()
        trade_embedding = self.model.encode(text, convert_to_tensor=True)
        
        # Cosine similarity to all training samples
        similarities = util.pytorch_cos_sim(trade_embedding, self.training_embeddings)[0]
        similarities = similarities.cpu().numpy()
        
        # Weighted average: higher weight to more similar examples
        # Use softmax to convert similarities to weights
        weights = np.exp(similarities) / np.sum(np.exp(similarities))
        score = np.dot(weights, self.training_scores)
        
        return float(score)
    
    def score_trades_batch(self, trades: List[Tuple[str, str]]) -> List[float]:
        """
        Score multiple trades efficiently.
        
        Args:
            trades: List of (title, outcome) tuples
        
        Returns:
            List of exposure scores in [0, 1]
        """
        texts = [f"On the market '{title}' I am betting '{outcome}'".strip() for title, outcome in trades]
        
        # Embed all at once (batched)
        trade_embeddings = self.model.encode(texts, convert_to_tensor=True)
        
        # Compute similarities for all trades
        similarities = util.pytorch_cos_sim(trade_embeddings, self.training_embeddings)
        
        # Weighted average for each trade
        scores = []
        for i in range(len(trades)):
            weights = np.exp(similarities[i].cpu().numpy()) / np.sum(np.exp(similarities[i].cpu().numpy()))
            score = float(np.dot(weights, self.training_scores))
            scores.append(score)
        
        return scores


def ensure_exposure_score_column(trades_db: str = "databases/trades.db"):
    """Add exposure_score column to trades table if it doesn't exist."""
    conn = sqlite3.connect(trades_db)
    cursor = conn.cursor()
    
    # Check if column exists
    cursor.execute("PRAGMA table_info(trades)")
    columns = [col[1] for col in cursor.fetchall()]
    
    if "exposure_score" not in columns:
        cursor.execute("ALTER TABLE trades ADD COLUMN exposure_score REAL DEFAULT NULL")
        conn.commit()
        print("Added exposure_score column to trades table")
    
    conn.close()


def score_unscored_trades(trades_db: str = "databases/trades.db", batch_size: int = 100):
    """
    Score all trades in trades.db that don't have an exposure score yet.
    
    Args:
        trades_db: Path to trades database
        batch_size: Number of trades to score at once
    """
    ensure_exposure_score_column(trades_db)
    
    scorer = ExposureScorer()
    
    conn = sqlite3.connect(trades_db)
    cursor = conn.cursor()
    
    # Find unscored trades
    cursor.execute(
        "SELECT transaction_hash, title, outcome FROM trades WHERE exposure_score IS NULL"
    )
    unscored = cursor.fetchall()
    
    if not unscored:
        print("No unscored trades found.")
        conn.close()
        return
    
    print(f"Scoring {len(unscored)} trades...")
    
    total = len(unscored)
    for i in range(0, len(unscored), batch_size):
        batch = unscored[i:i+batch_size]
        hashes = [row[0] for row in batch]
        trades = [(row[1], row[2]) for row in batch]
        
        # Score batch
        scores = scorer.score_trades_batch(trades)
        
        # Write back to DB
        for tx_hash, score in zip(hashes, scores):
            cursor.execute(
                "UPDATE trades SET exposure_score = ? WHERE transaction_hash = ?",
                (score, tx_hash)
            )
        
        conn.commit()
        print(f"  Scored {min(i + batch_size, total)} / {total}")
    
    conn.close()
    print("Done!")


if __name__ == "__main__":
    score_unscored_trades()
