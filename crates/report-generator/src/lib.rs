use orchyn_gap_detector::Gap;

pub struct ReportGenerator;

impl ReportGenerator {
    pub fn generate_markdown(gaps: &[Gap]) -> String {
        let mut report = String::from("# Orchyn Run Report\n\n");
        for gap in gaps {
            report.push_str(&format!("## {:?} Gap: {:?}\n", gap.severity, gap.kind));
            report.push_str(&format!("Expected: {}\n", gap.expected));
            report.push_str(&format!("Observed: {}\n\n", gap.observed));
        }
        report
    }
}
