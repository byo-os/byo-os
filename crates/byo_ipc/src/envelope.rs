use std::{
    any::TypeId,
    borrow::Cow,
    collections::HashMap,
    io::Cursor,
    num::NonZeroU128,
    sync::{OnceLock, RwLock},
};

use rmp_serde::Deserializer;
use serde::{Deserialize, Serialize};

use crate::base::IpcError;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IpcMessageType(Cow<'static, String>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpcMessageData(Vec<u8>);

impl IpcMessageData {
    pub fn from<T: Serialize>(message: &T) -> Result<Self, IpcError> {
        let mut serializer = rmp_serde::Serializer::new(Cursor::new(Vec::new()));
        message.serialize(&mut serializer)?;
        Ok(IpcMessageData(serializer.into_inner().into_inner()))
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub struct IpcPeer {
    id: NonZeroU128,
}

impl From<&str> for IpcMessageType {
    fn from(s: &str) -> Self {
        IpcMessageType(Cow::Owned(s.to_string()))
    }
}

pub struct IpcMeta {
    pub sender: Option<IpcPeer>,
    pub receiver: Option<IpcPeer>,
}

impl Default for IpcMeta {
    fn default() -> Self {
        IpcMeta {
            sender: None,
            receiver: None,
        }
    }
}

pub struct IpcEnvelope {
    pub meta: IpcMeta,
    pub message_type: IpcMessageType,
    pub message_data: IpcMessageData,
}

impl IpcEnvelope {
    pub fn try_new<T: Serialize + Deserialize<'static> + 'static>(
        message: &T,
    ) -> Result<Self, IpcError> {
        IpcRegistry::shared()
            .read()
            .or_else(|_| Err(IpcError::RegistryPoisoned))?
            .envelope(message, IpcMeta::default())
    }

    pub fn try_new_with_meta<T: Serialize + Deserialize<'static> + 'static>(
        message: &T,
        meta: IpcMeta,
    ) -> Result<Self, IpcError> {
        IpcRegistry::shared()
            .read()
            .or_else(|_| Err(IpcError::RegistryPoisoned))?
            .envelope(message, meta)
    }

    #[inline]
    pub fn when<'a, T: Serialize + Deserialize<'static> + 'static>(
        &'a self,
        handler: impl Fn(T) + 'a,
    ) -> &Self {
        match IpcRegistry::shared()
            .read()
            .unwrap()
            .message_type_for::<T>()
        {
            Some(message_type) => {
                if self.message_type != message_type {
                    return self;
                }
            }
            None => return self,
        }
        let mut deserializer = Deserializer::new(Cursor::new(&self.message_data.0));
        let message: T = Deserialize::deserialize(&mut deserializer).unwrap();
        handler(message);
        self
    }
}

pub struct IpcRegistry {
    message_types_by_type_id: HashMap<TypeId, IpcMessageType>,
    type_ids_by_message_type: HashMap<IpcMessageType, TypeId>,
}

impl IpcRegistry {
    pub fn new() -> Self {
        IpcRegistry {
            message_types_by_type_id: HashMap::new(),
            type_ids_by_message_type: HashMap::new(),
        }
    }

    pub fn shared() -> &'static std::sync::RwLock<Self> {
        static SINGLETON: OnceLock<RwLock<IpcRegistry>> = OnceLock::new();
        SINGLETON.get_or_init(|| RwLock::new(IpcRegistry::new()))
    }

    pub fn register<T: Serialize + Deserialize<'static> + 'static>(
        &mut self,
        message_type: impl Into<IpcMessageType>,
    ) -> &mut Self {
        let type_id = TypeId::of::<T>();
        let message_type = message_type.into();
        self.message_types_by_type_id
            .insert(type_id, message_type.clone());
        self.type_ids_by_message_type
            .insert(message_type.clone(), type_id);
        self
    }

    pub fn message_type_for<T: 'static>(&self) -> Option<IpcMessageType> {
        let type_id = TypeId::of::<T>();
        self.message_types_by_type_id.get(&type_id).cloned()
    }

    pub fn type_id_for(&self, message_type: IpcMessageType) -> Option<TypeId> {
        self.type_ids_by_message_type.get(&message_type).cloned()
    }

    pub fn envelope<T: Serialize + Deserialize<'static> + 'static>(
        &self,
        message: &T,
        meta: IpcMeta,
    ) -> Result<IpcEnvelope, IpcError> {
        let type_id = TypeId::of::<T>();
        let message_type = self
            .message_types_by_type_id
            .get(&type_id)
            .ok_or(IpcError::UnknownMessageType(
                std::any::type_name::<T>().to_string(),
            ))?
            .clone();
        let message_data = IpcMessageData::from(message)?;
        Ok(IpcEnvelope {
            meta,
            message_type,
            message_data,
        })
    }
}

pub struct IpcHandlerMux<'a> {
    handlers: HashMap<IpcMessageType, Box<dyn Fn(IpcEnvelope) -> Result<(), IpcError> + 'a>>,
}

impl<'a> IpcHandlerMux<'a> {
    pub fn new() -> Self {
        IpcHandlerMux {
            handlers: HashMap::new(),
        }
    }

    pub fn when<T: Serialize + Deserialize<'static> + 'static>(
        &mut self,
        handler: impl Fn(T, &IpcEnvelope) + 'a,
    ) -> &mut Self {
        let message_type = IpcRegistry::shared()
            .read()
            .unwrap()
            .message_type_for::<T>()
            .unwrap();
        self.handlers.insert(
            message_type,
            Box::new(move |envelope| {
                let mut deserializer = Deserializer::new(Cursor::new(&envelope.message_data.0));
                let message: T = Deserialize::deserialize(&mut deserializer)?;
                handler(message, &envelope);
                Ok(())
            }),
        );
        self
    }

    pub fn handle(&self, envelope: impl Into<IpcEnvelope>) -> Result<(), IpcError> {
        let envelope = envelope.into();
        let handler =
            self.handlers
                .get(&envelope.message_type)
                .ok_or(IpcError::UnknownMessageType(
                    envelope.message_type.0.clone().into_owned(),
                ))?;
        handler(envelope)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn test_ipc_registry() {
        IpcRegistry::shared()
            .write()
            .unwrap()
            .register::<String>("test")
            .register::<i32>("test2");

        assert_eq!(
            IpcRegistry::shared()
                .read()
                .unwrap()
                .message_type_for::<String>(),
            Some("test".into())
        );
        assert_eq!(
            IpcRegistry::shared()
                .read()
                .unwrap()
                .type_id_for("test".into()),
            Some(TypeId::of::<String>())
        );
    }

    #[test]
    fn test_envelope() {
        IpcRegistry::shared()
            .write()
            .unwrap()
            .register::<String>("test");
        let envelope = IpcRegistry::shared()
            .read()
            .unwrap()
            .envelope(&"hello".to_string(), IpcMeta::default())
            .unwrap();
        assert_eq!(envelope.message_type, "test".into());
        assert_eq!(
            envelope.message_data.0,
            vec![0xA5, 0x68, 0x65, 0x6C, 0x6C, 0x6F]
        );
    }

    #[test]
    fn test_handling() {
        IpcRegistry::shared()
            .write()
            .unwrap()
            .register::<String>("test");

        let envelope = IpcEnvelope::try_new(&"hello".to_string()).unwrap();
        let did_run = Cell::new(false);
        envelope.when(|message: String| {
            println!("{:?}", envelope.meta.sender);
            assert_eq!(message, "hello".to_string());
            assert_eq!(envelope.meta.sender, None);
            assert_eq!(envelope.meta.receiver, None);
            did_run.set(true);
        });
        std::mem::drop(envelope);
        assert!(did_run.into_inner());
    }

    #[test]
    fn test_mux() {
        IpcRegistry::shared()
            .write()
            .unwrap()
            .register::<String>("test");

        let mut mux = IpcHandlerMux::new();
        mux.when::<String>(|message: String, envelope: &IpcEnvelope| {
            assert_eq!(message, "hello".to_string());
            assert_eq!(envelope.meta.sender, None);
            assert_eq!(envelope.meta.receiver, None);
        });
        let envelope =
            IpcEnvelope::try_new_with_meta(&"hello".to_string(), IpcMeta::default()).unwrap();
        mux.handle(envelope).unwrap();
    }
}
