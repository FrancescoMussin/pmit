use crate::investigator::UserActivityReport;

const COL_USER: usize = 32;
const COL_DATE: usize = 35;
const COL_WINRATE: usize = 12;
const COL_PVALUE: usize = 12;
const COL_SAMPLE: usize = 15;

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        let mut truncated = s[..max_len - 3].to_string();
        truncated.push_str("...");
        truncated
    } else {
        format!("{:<width$}", s, width = max_len)
    }
}

pub fn print_investigation_header() {
    let top = format!(
        "┌{:─^width_user$}┬{:─^width_date$}┬{:─^width_winrate$}┬{:─^width_pvalue$}┬{:─^width_sample$}┐",
        "", "", "", "", "",
        width_user = COL_USER + 2,
        width_date = COL_DATE + 2,
        width_winrate = COL_WINRATE + 2,
        width_pvalue = COL_PVALUE + 2,
        width_sample = COL_SAMPLE + 2,
    );

    let header = format!(
        "│ {:<width_user$} │ {:<width_date$} │ {:<width_winrate$} │ {:<width_pvalue$} │ {:<width_sample$} │",
        "User", "Created At", "Win Rate", "p-value", "Sample Size",
        width_user = COL_USER,
        width_date = COL_DATE,
        width_winrate = COL_WINRATE,
        width_pvalue = COL_PVALUE,
        width_sample = COL_SAMPLE,
    );

    let sep = format!(
        "├{:─^width_user$}┼{:─^width_date$}┼{:─^width_winrate$}┼{:─^width_pvalue$}┼{:─^width_sample$}┤",
        "", "", "", "", "",
        width_user = COL_USER + 2,
        width_date = COL_DATE + 2,
        width_winrate = COL_WINRATE + 2,
        width_pvalue = COL_PVALUE + 2,
        width_sample = COL_SAMPLE + 2,
    );

    println!("\n[INVESTIGATION LOG]");
    println!("{}", top);
    println!("{}", header);
    println!("{}", sep);
}

pub fn print_investigation_row(report: &UserActivityReport) {
    let user = truncate(&report.public_profile.name, COL_USER);
    let date = truncate(report.public_profile.created_at.as_deref().unwrap_or("N/A"), COL_DATE);
    let win_rate = format!("{:.1}%", report.win_rate * 100.0);
    let p_value = format!("{:.4}", report.p_value);
    let sample = format!("{} trades", report.past_trades.len());

    let row = format!(
        "│ {:<width_user$} │ {:<width_date$} │ {:<width_winrate$} │ {:<width_pvalue$} │ {:<width_sample$} │",
        user, date, win_rate, p_value, sample,
        width_user = COL_USER,
        width_date = COL_DATE,
        width_winrate = COL_WINRATE,
        width_pvalue = COL_PVALUE,
        width_sample = COL_SAMPLE,
    );

    let sep = format!(
        "├{:─^width_user$}┼{:─^width_date$}┼{:─^width_winrate$}┼{:─^width_pvalue$}┼{:─^width_sample$}┤",
        "", "", "", "", "",
        width_user = COL_USER + 2,
        width_date = COL_DATE + 2,
        width_winrate = COL_WINRATE + 2,
        width_pvalue = COL_PVALUE + 2,
        width_sample = COL_SAMPLE + 2,
    );

    println!("{}", row);
    println!("{}", sep);
}
