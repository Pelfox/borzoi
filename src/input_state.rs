use std::collections::{HashMap, HashSet};

use smithay::backend::input::{Device, DeviceCapability};
use xkeysym::KeyCode;

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum KnownDeviceType {
    Unknown,
    Keyboard,
    Mouse,
}

impl KnownDeviceType {
    pub fn from_device<D: Device>(device: &D) -> Self {
        if device.has_capability(DeviceCapability::Keyboard) {
            KnownDeviceType::Keyboard
        } else if device.has_capability(DeviceCapability::Pointer) {
            KnownDeviceType::Mouse
        } else {
            KnownDeviceType::Unknown
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct KnownDevice {
    pub name: String,
    pub device_type: KnownDeviceType,
}

#[derive(Debug, Default)]
pub struct InputState {
    known_devices: HashMap<String, KnownDevice>,
    active_keyboard_keys: HashSet<KeyCode>,
}

impl InputState {
    pub fn on_device_added<D: Device>(&mut self, device: D) {
        let device_type = KnownDeviceType::from_device(&device);
        let known_device = KnownDevice {
            name: device.name(),
            device_type,
        };
        self.known_devices.insert(device.id(), known_device);
    }

    pub fn on_device_removed<D: Device>(&mut self, device: D) {
        self.known_devices.remove(&device.id());
    }

    // TODO(keystrokes): Maybe we should check for the keyboard too?
    pub fn on_keyboard_key_press(&mut self, key: KeyCode) {
        self.active_keyboard_keys.insert(key);
    }

    pub fn on_keyboard_key_release(&mut self, key: KeyCode) {
        self.active_keyboard_keys.remove(&key);
    }

    pub fn clear_keyboard_keys(&mut self) {
        self.active_keyboard_keys.clear();
    }

    pub fn is_keyboard_combination_pressed(&self, combination: Vec<KeyCode>) -> bool {
        println!("Active keyboard keys: {:?}", self.active_keyboard_keys);

        let mut is_pressed = true;
        for key in combination {
            if !self.active_keyboard_keys.contains(&key) {
                is_pressed = false;
                break;
            }
        }
        return is_pressed;
    }
}
