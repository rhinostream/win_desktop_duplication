//! This module contains [adapter (GPU)][Adapter] and [adapter factories][AdapterFactory] to acquire adapters.
//! The adapters can be used to enumerate various outputs connected to them.

use windows::core::{Interface, Result as WinResult};
use windows::Win32::Foundation::LUID;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, DXGI_ADAPTER_DESC, DXGI_ADAPTER_DESC3, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, IDXGIAdapter4, IDXGIFactory6};

use crate::outputs::Display;
use crate::utils::convert_u16_to_string;

#[cfg(test)]
mod test {
    use crate::devices::AdapterFactory;

    #[test]
    fn test_adapter_methods() {
        for adapter in AdapterFactory::new() {
            println!("{}", adapter.name());
            println!("{:?}", adapter.luid());
        }
    }

    #[test]
    fn test_adapter_factory() {
        for adapter in AdapterFactory::new() {
            println!("{}", adapter.name());
        }
    }
}

/**
Adapter object typically represents single gpu. It contains helpful methods for identifying
said gpu name, LUID and also allows for listing of outputs attached to given GPU.

Adapter is generated using [AdapterFactory].

* to iterate over attached displays, you can use [iter_displays][Adapter::iter_displays].
* to acquire a specific display, use [get_display_by_idx][Adapter::get_display_by_idx].
 */
#[repr(transparent)]
#[derive(Clone)]
pub struct Adapter(IDXGIAdapter4);

unsafe impl Send for Adapter {}

unsafe impl Sync for Adapter {}

impl Adapter {
    /// Returns name of the adapter
    pub fn name(&self) -> String {
        let mut desc: DXGI_ADAPTER_DESC3 = Default::default();
        unsafe { self.0.GetDesc3(&mut desc).unwrap() };
        convert_u16_to_string(&desc.Description)
    }

    /// returns LUID of the Adapter.
    pub fn luid(&self) -> LUID {
        let mut desc: DXGI_ADAPTER_DESC3 = Default::default();
        unsafe { self.0.GetDesc3(&mut desc).unwrap() };
        desc.AdapterLuid
    }

    /// returns DXGI Adapter reference.
    pub fn as_raw_ref(&self) -> &IDXGIAdapter4 {
        &self.0
    }

    /// returns an iterator for displays attached to this adapter
    /// ## Usage example:
    ///
    /// ```
    /// for display in adapter.iter_displays(){
    ///     // use the display object
    /// }
    /// ```
    pub fn iter_displays(&self) -> DisplayIterator {
        DisplayIterator::new(self.clone())
    }

    /// returns a specific display by index. if the item doesn't exist, returns `None`.
    pub fn get_display_by_idx(&self, idx: u32) -> Option<Display> {
        DisplayIterator::get_display_by_idx(&self, idx)
    }
}

/**
Display Iterator is used to iterate over displays attached to a particular [Adapter]. this
implements Iterator trait so it can be used in a for loop.

## Example usage:
```
use win_desktop_duplication::devices::{Adapter, AdapterFactory};
let adapter = AdapterFactory::new().get_adapter_by_idx(0);
 for display in adapter.iter_display(){
    // use display here
}
```

 */
#[repr(C)]
pub struct DisplayIterator {
    adapter: Adapter,
    idx: u32,
}

impl DisplayIterator {
    fn new(adapter: Adapter) -> Self {
        Self {
            adapter,
            idx: 0,
        }
    }
    fn get_display_by_idx(adapter: &Adapter, idx: u32) -> Option<Display> {
        let output = unsafe { adapter.0.EnumOutputs(idx) };
        if output.is_err() {
            None
        } else {
            Some(Display::new(output.unwrap().cast().unwrap()))
        }
    }
}

impl Iterator for DisplayIterator {
    type Item = Display;

    fn next(&mut self) -> Option<Self::Item> {
        let out = Self::get_display_by_idx(&self.adapter, self.idx);
        if out.is_some() {
            self.idx += 1;
        } else {
            self.idx = 0;
        }
        out
    }
}

/**AdapterFactory
Adapter factory is used to enumerate various adapters. It implements iterator. The iterator
state is auto reset when it reaches the end. you can also reset manually with [reset][AdapterFactory::reset] function.

```
use win_desktop_duplication::devices::AdapterFactory;
let mut fac = AdapterFactory::new();
for adapter in fac {
    // use adapter value here
}
```

you can also retrieve adapters by their specific index or LUID (unique identifier for current system)

```
use win_desktop_duplication::devices::AdapterFactory;
let mut fac = AdapterFactory::new();

// either
let adapter = fac.get_adapter_by_idx(0);
// or
let adapter = fac.get_adapter_by_luid(luid);
```

 */
pub struct AdapterFactory {
    fac: IDXGIFactory6,
    count: u32,
}

unsafe impl Send for AdapterFactory {}

unsafe impl Sync for AdapterFactory {}

impl Default for AdapterFactory {
    fn default() -> Self {
        AdapterFactory::new()
    }
}

impl AdapterFactory {
    /// Create new instance of AdapterFactory
    pub fn new() -> Self {
        unsafe {
            let dxgi_factory: IDXGIFactory6 = CreateDXGIFactory2(0).unwrap();
            Self {
                fac: dxgi_factory,
                count: 0,
            }
        }
    }

    /// retrieve an adapter by index
    pub fn get_adapter_by_idx(&self, idx: u32) -> Option<Adapter> {
        let adapter: WinResult<IDXGIAdapter4> = unsafe { self.fac.EnumAdapterByGpuPreference(idx, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE) };
        if adapter.is_ok() {
            Some(Adapter(adapter.unwrap().cast().unwrap()))
        } else {
            None
        }
    }

    /// retrieve an adapter by LUID
    pub fn get_adapter_by_luid(&self, luid: LUID) -> Option<Adapter> {
        let adapter = unsafe { self.fac.EnumAdapterByLuid(luid) };
        if adapter.is_ok() {
            Some(Adapter(adapter.unwrap()))
        } else {
            None
        }
    }

    /// reset the iterator status of AdapterFactory
    pub fn reset(&mut self) {
        self.count = 0;
    }

    /// acquire raw reference to IDXGIAdapterFactory
    pub fn as_raw_ref(&self) -> &IDXGIFactory6 {
        &self.fac
    }
}

impl Iterator for AdapterFactory {
    type Item = Adapter;

    fn next(&mut self) -> Option<Self::Item> {
        let adapter = self.get_adapter_by_idx(self.count);
        self.count += 1;
        if adapter.is_none() {
            self.count = 0;
        }
        adapter
    }
}
