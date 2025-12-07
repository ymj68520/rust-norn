use norn_common::traits::DBInterface;
use norn_common::types::Hash;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, broadcast};
use tracing::{debug, error, info};

// Constants
const TASK_CHANNEL_SIZE: usize = 10240;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTask {
    pub command_type: String, // "set" or "append"
    pub hash: Hash,
    pub height: i64,
    #[serde(with = "hex_serde")]
    pub address: Vec<u8>,
    #[serde(with = "hex_serde")]
    pub key: Vec<u8>,
    #[serde(with = "hex_serde")]
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub event_type: String,
    pub hash: String,
    pub height: String,
    pub address: String,
    pub params: HashMap<String, String>,
}

pub struct DataProcessor {
    task_tx: mpsc::Sender<DataTask>,
    // event_tx: broadcast::Sender<Event>, // For external subscribers (e.g. WS)
    // We hold the sender to clone it for others? Or just send internal events?
    // In Go, `CreateNewEventRouter` is a singleton.
    // Here we can expose a subscribe method or just public sender.
    pub event_tx: broadcast::Sender<Event>, 
    
    // Internal
    task_rx: tokio::sync::Mutex<mpsc::Receiver<DataTask>>, 
    db: Arc<dyn DBInterface>,
}

impl DataProcessor {
    pub fn new(db: Arc<dyn DBInterface>) -> Arc<Self> {
        let (task_tx, task_rx) = mpsc::channel(TASK_CHANNEL_SIZE);
        let (event_tx, _) = broadcast::channel(256); // 256 buffer size

        let processor = Arc::new(Self {
            task_tx,
            event_tx,
            task_rx: tokio::sync::Mutex::new(task_rx),
            db,
        });

        let p = processor.clone();
        tokio::spawn(async move {
            p.run().await;
        });

        processor
    }
    
    pub async fn submit_task(&self, task: DataTask) {
        if let Err(e) = self.task_tx.send(task).await {
            error!("Failed to submit data task: {}", e);
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.event_tx.subscribe()
    }

    async fn run(&self) {
        let mut rx = self.task_rx.lock().await;
        while let Some(task) = rx.recv().await {
            if task.command_type == "set" {
                self.set_data(&task).await;
            } else if task.command_type == "append" {
                 // Validate if value is JSON
                if serde_json::from_slice::<HashMap<String, String>>(&task.value).is_err() {
                     error!("Receive append task failed: value is not a string-map json");
                     continue;
                }
                self.append_data(&task).await;
            }
        }
    }

    async fn set_data(&self, task: &DataTask) {
        let db_key = norn_common::utils::db_keys::data_address_key_to_db_key(&task.address, &task.key);
        
        // Go logic: try unmarshal to map. If ok, append to array?
        // Wait, "set" logic in Go:
        // `err := json.Unmarshal(task.Value, &mapValue)`
        // `if err == nil { mapArray = append(mapArray, mapValue); value, _ = json.Marshal(mapArray) }`
        // So "set" ALSO treats it as an array if it looks like a map? 
        // This seems like specific logic: "If input is map, wrap in array. Else just raw bytes".
        // Go `set` command seems to behave like `append` if input is JSON map?
        // Let's re-read Go `setData`:
        /*
            err := json.Unmarshal(task.Value, &mapValue)
            if err != nil {
                value = task.Value
            } else {
                mapArray = append(mapArray, mapValue)
                value, _ = json.Marshal(mapArray)
            }
            _ = db.Insert(dbKey, value) // task.Value passed to Insert in Go? 
            // `_ = db.Insert(dbKey, task.Value)` <-- ERROR in Go code?
            // Go Code: `_ = db.Insert(dbKey, task.Value)`
            // BUT logging says `string(value)` which is the NEW value.
            // AND params uses `string(value)`.
            // BUT DB Insert uses `task.Value`.
            // If Go code inserts `task.Value` (raw), then the array wrapping logic is IGNORED for DB?
            // But used for Event?
            // This looks like a bug in Go code or I am misreading `task.Value` vs `value`.
            // Go: `value` variable is derived. `task.Value` is original.
            // Go Line: `_ = db.Insert(dbKey, task.Value)`
            // It writes ORIGINAL value to DB.
            // But log says `Trying insert ... value %s` (using derived value).
            // AND Event uses derived value.
            
            // IF this is a "Set" command, usually it overwrites.
            // If I fix it to write `value` (wrapped), I might break compat?
            // But if Go writes `task.Value`, then `set` just overwrites with input.
            // The JSON parsing is just for... the event?
            // I will match Go's DB behavior: Insert `task.Value`.
        */
        
        let write_value = &task.value; // Match Go's DB write
        // But for event params, we need the "processed" value logic?
        
        let mut display_value_str = String::from_utf8_lossy(&task.value).to_string();

        if let Ok(map_val) = serde_json::from_slice::<HashMap<String, String>>(&task.value) {
            let arr = vec![map_val];
            if let Ok(json_arr) = serde_json::to_string(&arr) {
                display_value_str = json_arr;
            }
        }

        if let Err(e) = self.db.insert(&db_key, write_value).await {
            error!("DB Insert failed: {}", e);
        } else {
            info!("Insert data key={} value={}", hex::encode(&db_key), display_value_str);
        }

        self.emit_event(task, display_value_str);
    }

    async fn append_data(&self, task: &DataTask) {
        let db_key = norn_common::utils::db_keys::data_address_key_to_db_key(&task.address, &task.key);
        
        let map_value: HashMap<String, String> = match serde_json::from_slice(&task.value) {
            Ok(v) => v,
            Err(_) => return, // Should be checked before
        };

        let mut map_array: Vec<HashMap<String, String>> = Vec::new();
        
        // Read existing
        if let Ok(Some(existing_bytes)) = self.db.get(&db_key).await {
             if let Ok(arr) = serde_json::from_slice(&existing_bytes) {
                 map_array = arr;
             }
        }
        
        map_array.push(map_value);
        
        let new_val_bytes = match serde_json::to_vec(&map_array) {
            Ok(v) => v,
            Err(e) => {
                error!("Marshal failed: {}", e);
                return;
            }
        };

        if let Err(e) = self.db.insert(&db_key, &new_val_bytes).await {
             error!("DB Append failed: {}", e);
        } else {
             info!("Append data key={} value={}", hex::encode(&db_key), String::from_utf8_lossy(&new_val_bytes));
        }
        
        self.emit_event(task, String::from_utf8_lossy(&new_val_bytes).to_string());
    }

    fn emit_event(&self, task: &DataTask, value_str: String) {
        let mut params = HashMap::new();
        params.insert("key".to_string(), String::from_utf8_lossy(&task.key).to_string());
        params.insert("value".to_string(), value_str);

        let event = Event {
            event_type: "data".to_string(),
            hash: hex::encode(task.hash.0),
            height: task.height.to_string(),
            address: hex::encode(&task.address),
            params,
        };

        let _ = self.event_tx.send(event);
    }
}

mod hex_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        serializer.serialize_str(&hex::encode(bytes))
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where D: Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        hex::decode(s).map_err(serde::de::Error::custom)
    }
}
