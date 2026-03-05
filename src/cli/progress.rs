use std::time::Instant;

pub struct ProgressIndicator {
    total: usize,
    completed: usize,
    failed: usize,
    start_time: Instant,
}

impl ProgressIndicator {
    pub fn new(total: usize) -> Self {
        Self {
            total,
            completed: 0,
            failed: 0,
            start_time: Instant::now(),
        }
    }

    pub fn start_item(&self, name: &str) {
        println!(
            "Processing: {} ({}/{})",
            name,
            self.completed + self.failed + 1,
            self.total
        );
    }

    pub fn complete_item(&mut self, _name: &str, success: bool) {
        if success {
            self.completed += 1;
        } else {
            self.failed += 1;
        }
    }

    pub fn finish(&self) {
        let elapsed = self.start_time.elapsed();
        println!("\n{}", "=".repeat(60));
        println!("Summary:");
        println!("  Total:     {}", self.total);
        println!("  Succeeded: {}", self.completed);
        println!("  Failed:    {}", self.failed);
        println!("  Duration:  {:.2}s", elapsed.as_secs_f64());
        println!("{}", "=".repeat(60));
    }
}
