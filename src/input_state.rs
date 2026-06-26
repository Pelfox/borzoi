use std::collections::HashMap;

use smithay::{
    backend::input::{Device, DeviceCapability},
    input::{Seat, SeatHandler, SeatState, keyboard::KeyboardHandle, pointer::PointerHandle},
    wayland::seat::{SeatGlobalData, WaylandFocus},
};
use wayland_server::{DisplayHandle, GlobalDispatch, protocol::wl_seat::WlSeat};

#[derive(PartialEq, Eq)]
pub enum KnownDeviceType {
    Keyboard,
    Mouse,
}

pub struct KnownDevice<S: SeatHandler> {
    pub name: String,
    pub keyboard_handle: Option<KeyboardHandle<S>>,
    pub pointer_handle: Option<PointerHandle<S>>,
}

pub struct InputState<S: SeatHandler> {
    seat: Seat<S>,
    known_devices: HashMap<String, KnownDevice<S>>,
}

impl<S> InputState<S>
where
    S: SeatHandler + 'static,
    <S as SeatHandler>::KeyboardFocus: WaylandFocus,
    <S as SeatHandler>::PointerFocus: WaylandFocus,
    S: GlobalDispatch<WlSeat, SeatGlobalData<S>>,
{
    pub fn new(display_handle: &DisplayHandle, seat_state: &mut SeatState<S>) -> Self {
        Self {
            seat: seat_state.new_wl_seat(&display_handle, "seat-0"),
            known_devices: HashMap::default(),
        }
    }

    pub fn on_device_added<D: Device>(&mut self, device: D) -> anyhow::Result<()> {
        let mut known_device = KnownDevice {
            name: device.name(),
            keyboard_handle: None,
            pointer_handle: None,
        };

        if device.has_capability(DeviceCapability::Keyboard) {
            let keyboard_handle = self.seat.add_keyboard(Default::default(), 200, 25).ok();
            known_device.keyboard_handle = keyboard_handle;
        }

        if device.has_capability(DeviceCapability::Pointer) {
            known_device.pointer_handle = Some(self.seat.add_pointer());
        }

        self.known_devices.insert(device.id(), known_device);
        Ok(())
    }

    pub fn on_device_removed<D: Device>(&mut self, device: D) {
        self.known_devices.remove(&device.id());
    }

    pub fn pointer_handle_for_device<D: Device>(
        &self,
        device: D,
    ) -> anyhow::Result<PointerHandle<S>> {
        let known_device = self
            .known_devices
            .get(&device.id())
            .ok_or(anyhow::anyhow!("device is not registered"))?;
        if let Some(pointer_handle) = &known_device.pointer_handle {
            return Ok(pointer_handle.clone());
        } else {
            anyhow::bail!("given device is not a mouse")
        }
    }

    pub fn keyboard_handle_for_device<D: Device>(
        &self,
        device: D,
    ) -> anyhow::Result<KeyboardHandle<S>> {
        let known_device = self
            .known_devices
            .get(&device.id())
            .ok_or(anyhow::anyhow!("device is not registered"))?;
        if let Some(keyboard_handle) = &known_device.keyboard_handle {
            return Ok(keyboard_handle.clone());
        } else {
            anyhow::bail!("given device is not a keyboard")
        }
    }
}
