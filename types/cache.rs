trait Cache {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
}