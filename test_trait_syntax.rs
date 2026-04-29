// Minimal test file to find correct syntax for Box<dyn Future<...>>
use std::future::Future;

pub type Result<T> = std::result::Result<T, String>;

pub trait TestTrait: Send + Sync {
    fn foo(&self) -> Box<dyn Future<Output = Result<()>> + Send + '_>;
}

struct TestStruct;

impl TestTrait for TestStruct {
    fn foo(&self) -> Box<dyn Future<Output = Result<()>> + Send + '_> {
        Box::new(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_trait() {
        // This should compile
    }
}
