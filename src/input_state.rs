use std::collections::HashMap;

use smithay::{
    backend::input::{Device, DeviceCapability},
    input::{Seat, SeatHandler, SeatState, keyboard::KeyboardHandle, pointer::PointerHandle},
    wayland::seat::{SeatGlobalData, WaylandFocus},
};
use wayland_server::{DisplayHandle, GlobalDispatch, protocol::wl_seat::WlSeat};

/// Describes a single known device to the compositor, providing underlying
/// handles for it.
pub struct KnownDevice<S: SeatHandler> {
    /// Human-friendly name of the device.
    pub name: String,
    /// Device's keyboard handle, if any. Applicable only for keyboards.
    pub keyboard_handle: Option<KeyboardHandle<S>>,
    /// Device's pointer (mouse) handle, if any. Applicable only for mice.
    pub pointer_handle: Option<PointerHandle<S>>,
}

/// Describes current compositor's input devices state.
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
    /// Creates a new compositor's input state, acquiring the first available
    /// Wayland seat.
    pub fn new(display_handle: &DisplayHandle, seat_state: &mut SeatState<S>) -> Self {
        Self {
            seat: seat_state.new_wl_seat(&display_handle, "seat-0"),
            known_devices: HashMap::default(),
        }
    }

    /// Handles device addition.
    pub fn on_device_added<D>(&mut self, device: &D) -> anyhow::Result<()>
    where
        D: Device,
    {
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

    /// Handles device remove.
    pub fn on_device_removed<D>(&mut self, device: &D)
    where
        D: Device,
    {
        self.known_devices.remove(&device.id());
    }

    /// Retrieves keyboard handle for the given device.
    pub fn keyboard_handle_for_device<D>(&self, device: &D) -> anyhow::Result<KeyboardHandle<S>>
    where
        D: Device,
    {
        let device_id = device.id();
        let known_device = self
            .known_devices
            .get(&device_id)
            .ok_or(anyhow::anyhow!("unknown device {}", device_id))?;
        known_device
            .keyboard_handle
            .clone()
            .ok_or(anyhow::anyhow!("device is not a keyboard"))
    }

    /// Retrieves pointer (mouse) handle for the given device.
    pub fn pointer_handle_for_device<D>(&self, device: &D) -> anyhow::Result<PointerHandle<S>>
    where
        D: Device,
    {
        let device_id = device.id();
        let known_device = self
            .known_devices
            .get(&device_id)
            .ok_or(anyhow::anyhow!("unknown device {}", device_id))?;
        known_device
            .pointer_handle
            .clone()
            .ok_or(anyhow::anyhow!("device is not a mouse"))
    }

    /// Retrieves the keyboard connected to the current seat.
    pub fn get_keyboard(&self) -> Option<KeyboardHandle<S>> {
        self.seat.get_keyboard().clone()
    }
}
