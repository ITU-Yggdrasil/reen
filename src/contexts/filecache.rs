use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct FileCache {
    folder: Option<PathBuf>,
    cache: Mutex<HashMap<String, PathBuf>>,
}

impl FileCache {
    pub fn new(folder: Option<String>) -> Self {
        FileCache {
            folder: folder.map(PathBuf::from),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn add(&self, key: String, value: PathBuf) -> Result<(), String> {
        let mut cache = self.cache.lock().unwrap();
        cache.insert(key, value);
        Ok(())
    }

    pub fn get(&self, key: String) -> Result<PathBuf, String> {
        let cache = self.cache.lock().unwrap();
        cache.get(&key).cloned().ok_or_else(|| format!("Key not found: {}", key))
    }

    pub fn remove(&self, key: String) -> Result<(), String> {
        let mut cache = self.cache.lock().unwrap();
        cache.remove(&key).map_err(|_| format!("Key not found: {}", key))
    }

    pub fn flush(&self) {
        self.cache.lock().unwrap().clear();
    }
}

#[derive(Debug, Clone)]
pub struct AgentInstructions {
    // Define the structure of agent instructions
    // For example:
    // pub agent_id: u32,
    // pub model_name: String,
    // pub input_json: serde_json::Value,
    // ...
}

impl AgentInstructions {
    pub fn hash(&self) -> String {
        // Implement hash logic for instructions
        // For example:
        // format!("{:?}", self)
        format!("{:?}", self)
    }
}

impl FileCache {
    pub fn cache_path(&self, instructions: &AgentInstructions, input_json: &serde_json::Value) -> PathBuf {
        let instructions_model_hash = format!("{:?}", instructions);
        let input_hash = serde_json::json(input_json).to_string();
        let hash = format!("{}/{}", instructions_model_hash, input_hash);
        let mut hasher = sha2::Sha256::new();
        hasher.update(&hash);
        let instructions_model_hash = hasher.finalize().to_vec();
        let instructions_model_hash_str = hex::encode(instructions_model_hash);

        let mut hasher = sha2::Sha256::new();
        hasher.update(&serde_json::json(input_json).to_string());
        let input_hash = hasher.finalize().to_vec();
        let input_hash_str = hex::encode(input_hash);

        let folder_path = self.folder.as_ref().unwrap_or(&Path::new(".reen"));
        let file_path = folder_path.join(format!("{}/{}", instructions_model_hash_str, input_hash_str)).with_extension("cache");
        file_path
    }

    pub fn cache_key(&self, instructions: &AgentInstructions, input_json: &serde_json::Value) -> String {
        format!("{}/{}", FileCache::hash(&instructions), serde_json::json(input_json).to_string())
    }
}

impl FileCache {
    pub fn instructions_model_hash(&self, instructions: &AgentInstructions) -> String {
        FileCache::hash(instructions)
    }

    pub fn input_hash(&self, input_json: &serde_json::Value) -> String {
        serde_json::json(input_json).to_string()
    }
}