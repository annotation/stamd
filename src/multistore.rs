use crate::common::ApiError;
use stam::{AnnotationStore, Config};
use std::collections::HashMap;
use std::path::{Component, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::info;

const WAIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct StoreState {
    last_access: Duration,
    /// Flag set when data is still being loaded from disk
    loading: bool,

    /// FlagFlag set when data is still being written from disk
    saving: bool,
}

pub struct StorePool {
    basedir: PathBuf,
    extension: String,
    readonly: bool,
    unload_time: u64,
    stores: RwLock<HashMap<String, Arc<RwLock<AnnotationStore>>>>, //the extra Arc allows us to drop the lock earlier
    states: RwLock<HashMap<String, StoreState>>,
    config: Config,
}

impl StorePool {
    pub fn new(
        basedir: impl Into<PathBuf>,
        extension: impl Into<String>,
        readonly: bool,
        unload_time: u64,
        config: Config,
    ) -> Result<Self, &'static str> {
        let basedir: PathBuf = basedir.into();
        if !basedir.is_dir() {
            Err("Base directory must exist")
        } else {
            Ok(Self {
                basedir,
                extension: extension.into(),
                stores: HashMap::new().into(),
                states: HashMap::new().into(),
                unload_time,
                readonly,
                config,
            })
        }
    }

    pub fn map<F, T>(&self, id: &str, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&AnnotationStore) -> Result<T, ApiError>,
    {
        let _state = self.load(id)?;
        if let Ok(stores) = self.stores.read() {
            if let Some(store) = stores.get(id).cloned() {
                drop(stores); //compiler should be able to infer this but better safe than sorry
                if let Ok(store) = store.read() {
                    f(&store)
                } else {
                    Err(ApiError::InternalError("Store lock got poisoned")) //only happens if a thread holding a write lock panics
                }
            } else {
                Err(ApiError::InternalError("Annotationstore not loaded"))
            }
        } else {
            Err(ApiError::InternalError("Lock poisoned: stores"))
        }
    }

    pub fn map_mut<F, T>(&self, id: &str, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&mut AnnotationStore) -> Result<T, ApiError>,
    {
        if self.readonly {
            Err(ApiError::PermissionDenied(
                "Service is configured as read-only",
            ))
        } else {
            let _state = self.load(id)?;
            if let Ok(stores) = self.stores.write() {
                if let Some(store) = stores.get(id).cloned() {
                    drop(stores); //compiler should be able to infer this but better safe than sorry
                    if let Ok(mut store) = store.write() {
                        f(&mut store)
                    } else {
                        Err(ApiError::InternalError("Store lock got poisoned")) //only happens if a thread holding a write lock panics
                    }
                } else {
                    Err(ApiError::InternalError("Annotationstore not loaded"))
                }
            } else {
                Err(ApiError::InternalError("Lock poisoned: stores"))
            }
        }
    }

    /// Loads an annotation store if it is not already loaded.
    /// Only one thread can load at a time.
    /// This function blocks until the store is loaded (either by us or by another thread)
    /// Returns a **copy** of the state
    fn load(&self, id: &str) -> Result<StoreState, ApiError> {
        let mut loading: Option<bool> = None;

        //loop in case we have to wait for another thread to do the loading
        loop {
            if let Ok(states) = self.states.read() {
                if let Some(state) = states.get(id) {
                    loading = Some(state.loading);
                }
            } else {
                return Err(ApiError::InternalError("Lock poisoned"));
            }
            match loading {
                Some(true) => {
                    //already loading in another thread
                    std::thread::sleep(WAIT_INTERVAL);
                }
                Some(false) => {
                    //already loaded, we update the access time only
                    if let Ok(mut states) = self.states.write() {
                        if let Some(state) = states.get_mut(id) {
                            state.last_access =
                                SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                            return Ok(state.clone());
                        } else {
                            return Err(ApiError::InternalError("State must exist"));
                        }
                    } else {
                        return Err(ApiError::InternalError("Lock poisoned"));
                    }
                }
                None => break, //not loaded yet
            }
        }

        let filename: PathBuf = id.into();

        //some security checks so the user can't break out of the configured base directory
        if filename.is_absolute() {
            return Err(ApiError::NotFound(
                "No such annotationstore exists (no absolute paths allowed)",
            ));
        }
        for component in filename.components() {
            if component == Component::ParentDir {
                return Err(ApiError::NotFound(
                    "No such annotationstore exists (no parent directories allowed)",
                ));
            }
        }

        let filename = self
            .basedir
            .clone()
            .join(filename)
            .with_extension(&self.extension);
        if !filename.exists() {
            return Err(ApiError::NotFound("No such annotationstore exists"));
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        if let Ok(mut states) = self.states.write() {
            //mark as loading
            states.insert(
                id.to_string(),
                StoreState {
                    last_access: now,
                    loading: true,
                    saving: false,
                },
            );
        } else {
            return Err(ApiError::InternalError("Lock poisoned"));
        }

        //note the actual store loading (time intensive) done here is done without any locks held
        if let Some(filename) = filename.to_str() {
            info!("Loading {}", id);
            match AnnotationStore::from_file(filename, self.config.clone()) {
                Ok(store) => {
                    //TODO: verify substores and resources can't break out of the base dir either!
                    if let Ok(mut stores) = self.stores.write() {
                        stores.insert(id.to_string(), Arc::new(RwLock::new(store)));
                    } else {
                        return Err(ApiError::InternalError("Lock poisoned"));
                    }
                }
                Err(e) => {
                    return Err(ApiError::StamError(e));
                }
            }
        } else {
            return Err(ApiError::NotFound(
                "No such annotationstore exists (invalid unicode)",
            ));
        }

        //mark loading as done:
        if let Ok(mut states) = self.states.write() {
            if let Some(state) = states.get_mut(id) {
                state.loading = false;
                Ok(state.clone())
            } else {
                return Err(ApiError::InternalError("State must exist"));
            }
        } else {
            return Err(ApiError::InternalError("Lock poisoned"));
        }
    }

    fn wait_until_ready(&self, id: &str) -> Result<StoreState, ApiError> {
        //loop in case we have to wait for another thread to do loading or saving
        let mut wait = false;
        loop {
            if let Ok(states) = self.states.read() {
                if let Some(state) = states.get(id) {
                    wait = state.loading || state.saving;
                    if !wait {
                        return Ok(state.clone());
                    }
                }
            } else {
                return Err(ApiError::InternalError("Lock poisoned"));
            }
            if wait {
                std::thread::sleep(WAIT_INTERVAL);
            } else {
                return Err(ApiError::NotFound("No such store loaded"));
            }
        }
    }

    /// Save an annotation store to disk if there are any changes
    /// Will return an error if the store is not loaded
    pub fn save(&self, id: &str) -> Result<(), ApiError> {
        self.wait_until_ready(id)?;

        if self.readonly {
            return Err(ApiError::PermissionDenied(
                "Service is configured as read-only",
            ));
        }

        //mark in progress
        if let Ok(mut states) = self.states.write() {
            if let Some(state) = states.get_mut(id) {
                state.saving = true;
            } else {
                return Err(ApiError::InternalError("State must exist"));
            }
        } else {
            return Err(ApiError::InternalError("Lock poisoned"));
        }

        if let Ok(stores) = self.stores.read() {
            if let Some(store) = stores.get(id).cloned() {
                drop(stores); //compiler should be able to infer this, but better safe than sorry
                if let Ok(store) = store.read() {
                    //read lock held during saving, so nothing else can write
                    if store.changed() {
                        info!("Saving {}", id);
                        store.save()?;
                    }
                }
            }
        }

        //mark done
        if let Ok(mut states) = self.states.write() {
            if let Some(state) = states.get_mut(id) {
                state.saving = false;
            } else {
                return Err(ApiError::InternalError("State must exist"));
            }
        } else {
            return Err(ApiError::InternalError("Lock poisoned"));
        }

        Ok(())
    }

    /// Unload an annotation store if it is loaded (no-op if it isn't loaded)
    pub fn unload(&self, id: &str) -> Result<(), ApiError> {
        match self.wait_until_ready(id) {
            Ok(_) => {
                if !self.readonly {
                    self.save(id)?;
                }
                if let Ok(mut stores) = self.stores.write() {
                    if stores.contains_key(id) {
                        stores.remove(id);
                    }
                } else {
                    return Err(ApiError::InternalError("Lock poisoned"));
                }

                if let Ok(mut states) = self.states.write() {
                    if states.contains_key(id) {
                        states.remove(id);
                    }
                } else {
                    return Err(ApiError::InternalError("Lock poisoned"));
                }

                info!("Unloaded {}", id);
                Ok(())
            }
            Err(ApiError::NotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn flush(&self, force: bool) -> Result<Vec<String>, ApiError> {
        let mut remove_ids: Vec<String> = Vec::new();

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        if let Ok(states) = self.states.read() {
            for (id, state) in states.iter() {
                if force || (now - state.last_access).as_secs() >= self.unload_time {
                    remove_ids.push(id.to_string());
                }
            }
        } else {
            return Err(ApiError::InternalError("Lock poisoned"));
        }

        for id in remove_ids.iter() {
            self.unload(&id)?;
        }

        Ok(remove_ids)
    }
}

impl Drop for StorePool {
    fn drop(&mut self) {
        if !self.readonly {
            self.flush(true).expect("Clean shutdown failed");
        }
    }
}
