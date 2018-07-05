use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};

use wayland_commons::map::ObjectMap;
use wayland_commons::wire::Message;

use super::connection::{Connection, Error as CError};
use super::proxy::{ObjectMeta, ProxyInner};

pub(crate) type QueueBuffer = Arc<Mutex<VecDeque<Message>>>;

pub(crate) fn create_queue_buffer() -> QueueBuffer {
    Arc::new(Mutex::new(VecDeque::new()))
}

pub(crate) struct EventQueueInner {
    pub(crate) connection: Arc<Mutex<Connection>>,
    pub(crate) map: Arc<Mutex<ObjectMap<ObjectMeta>>>,
    pub(crate) buffer: QueueBuffer,
    display_buffer: QueueBuffer,
}

impl EventQueueInner {
    pub(crate) fn new(connection: Arc<Mutex<Connection>>, buffer: Option<QueueBuffer>) -> EventQueueInner {
        let (map, display_buffer) = {
            let mut cx = connection.lock().unwrap();
            (cx.map.clone(), cx.display_buffer.clone())
        };
        EventQueueInner {
            connection,
            map,
            buffer: buffer.unwrap_or_else(create_queue_buffer),
            display_buffer,
        }
    }

    pub(crate) fn dispatch(&self) -> io::Result<u32> {
        // don't read events if there are some pending
        if let Err(()) = self.prepare_read() {
            return self.dispatch_pending();
        }

        self.connection.lock().unwrap().flush().map_err(|e| match e {
            ::nix::Error::Sys(errno) => io::Error::from(errno),
            _ => unreachable!(),
        })?;

        // TODO: block on wait for read readiness before reading
        self.read_events()?;

        self.dispatch_pending()
    }

    fn dispatch_buffer(&self, buffer: &mut VecDeque<Message>) -> io::Result<u32> {
        let mut count = 0;
        let mut proxymap = super::ProxyMap::make(self.map.clone(), self.connection.clone());
        for msg in buffer.drain(..) {
            let id = msg.sender_id;
            if let Some(proxy) = ProxyInner::from_id(id, self.map.clone(), self.connection.clone()) {
                let object = proxy.object.clone();
                let mut dispatcher = object.meta.dispatcher.lock().unwrap();
                if let Err(()) = dispatcher.dispatch(msg, proxy, &mut proxymap) {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Dispatch for object {}@{} errored.", object.interface, id),
                    ));
                } else {
                    count += 1;
                }
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Received an event for unknown object {}.", id),
                ));
            }
        }
        Ok(count)
    }

    pub(crate) fn dispatch_pending(&self) -> io::Result<u32> {
        // First always dispatch the display buffer
        let display_dispatched = {
            let mut buffer = self.display_buffer.lock().unwrap();
            self.dispatch_buffer(&mut *buffer)
        }?;

        // Then our actual buffer
        let self_dispatched = {
            let mut buffer = self.buffer.lock().unwrap();
            self.dispatch_buffer(&mut *buffer)
        }?;

        Ok(display_dispatched + self_dispatched)
    }

    pub(crate) fn sync_roundtrip(&self) -> io::Result<i32> {
        unimplemented!()
    }

    pub(crate) fn prepare_read(&self) -> Result<(), ()> {
        // TODO: un-mock
        Ok(())
    }

    pub(crate) fn read_events(&self) -> io::Result<i32> {
        // TODO: integrate more properly with prepare read with a fence
        match self.connection.lock().unwrap().read_events() {
            Ok(n) => Ok(n as i32),
            Err(CError::Protocol) => Err(::nix::errno::Errno::EPROTO.into()),
            Err(CError::Parse(_)) => Err(::nix::errno::Errno::EPROTO.into()),
            Err(CError::Nix(::nix::Error::Sys(errno))) => Err(errno.into()),
            Err(CError::Nix(_)) => unreachable!(),
        }
    }

    pub(crate) fn cancel_read(&self) {
        // TODO: un-mock
    }
}
