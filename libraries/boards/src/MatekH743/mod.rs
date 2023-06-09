extern crate alloc;

use crate::hal::{
    device::UART7,
    gpio::*,
    pac,
    pac::interrupt,
    prelude::*,
    rcc::rec::UsbClkSel,
    serial::Serial,
    timer,
    usb_hs::{UsbBus, USB2},
};
use alloc::sync::Arc;
use core::{
    borrow::Borrow,
    cell::RefCell,
    option::{Option, Option::*},
    ptr::null_mut,
    result::Result::{Err, Ok},
    sync::atomic::{
        AtomicPtr, AtomicU32,
        Ordering::{Relaxed, SeqCst},
    },
};
use cortex_m::interrupt::Mutex;
use lazy_static::lazy_static;
use usb_device::prelude::*;
use usbd_serial::SerialPort;

type led_blue_type = Pin<'E', 3, Output<PushPull>>;
type led_green_type = Pin<'E', 4, Output<PushPull>>;

pub struct USBREF {}

pub struct USB<'a> {
    serial: SerialPort<'a, UsbBus<USB2>>,
    device: UsbDevice<'a, UsbBus<USB2>>,
}

pub struct HALDATA {
    pub led_blue: AtomicPtr<led_blue_type>,
    pub led_green: AtomicPtr<led_green_type>,
    pub telem1: AtomicPtr<Serial<UART7>>,
    pub usb: freertos_rust::Mutex<USB<'static>>,
}

pub trait ExtU16 {
    fn bytes_to_words(self) -> u16;
    fn words_to_bytes(self) -> u16;
}

impl ExtU16 for u16 {
    #[inline]
    fn bytes_to_words(self) -> u16 {
        self / 4
    }
    #[inline]
    fn words_to_bytes(self) -> u16 {
        self * 4
    }
}

use core::fmt;

static TIMER: Mutex<RefCell<Option<timer::Timer<pac::TIM2>>>> = Mutex::new(RefCell::new(None));
static OVERFLOWS: AtomicU32 = AtomicU32::new(0);
static TIM2_CALLBACK: AtomicPtr<fn() -> ()> = AtomicPtr::new(null_mut());

// #[macro_export]
// macro_rules! console_print {
//     ($($arg:tt)*) => ($crate::MatekH743::_print(format_args!($($arg)*)));
// }
//
// #[macro_export]
// macro_rules! console_println {
//     () => ($crate::console_print!("\n"));
//     ($($arg:tt)*) => ($crate::console_print!("{}\n", format_args!($($arg)*)));
// }
//
static mut USB_BUF_IN: [u8; 64] = [0; 64];

impl<'a> USB<'a> {
    fn print(&mut self, args: fmt::Arguments) {
        let string = alloc::format!("{}", args);
        let buf = string.as_bytes();
        let mut write_offset = 0;
        let count = buf.len();
        match self.serial.write(&buf[write_offset..count]) {
            Ok(len) if len > 0 => {
                write_offset += len;
            }
            _ => {}
        }
    }

    fn read(&mut self) {
        unsafe {
            self.serial.read(&mut USB_BUF_IN);
        }
    }

    fn poll(&mut self) -> bool {
        self.device.poll(&mut [&mut self.serial])
    }
}

static mut EP_MEMORY: [u32; 1024] = [0; 1024];

impl HALDATA {
    fn new() -> Self {
        let mut cp = cortex_m::Peripherals::take().unwrap();
        let dp = pac::Peripherals::take().unwrap();
        let pwrcfg = dp.PWR.constrain().freeze();
        let rcc = dp.RCC.constrain();
        let mut ccdr = rcc.sys_ck(80.MHz()).freeze(pwrcfg, &dp.SYSCFG);
        let _ = ccdr.clocks.hsi48_ck().expect("HSI48 must run");
        ccdr.peripheral.kernel_usb_clk_mux(UsbClkSel::Hsi48);

        let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);
        let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);

        let mut led_blue = gpioe.pe3.into_push_pull_output();
        let mut led_green = gpioe.pe4.into_push_pull_output();
        led_blue.set_high();
        led_green.set_high();

        let rx = gpioe.pe7.into_alternate::<7>();
        let tx = gpioe.pe8.into_alternate::<7>();
        let mut telem1 = dp
            .UART7
            .serial((tx, rx), 57_600.bps(), ccdr.peripheral.UART7, &ccdr.clocks)
            .unwrap();

        let usb = USB2::new(
            dp.OTG2_HS_GLOBAL,
            dp.OTG2_HS_DEVICE,
            dp.OTG2_HS_PWRCLK,
            gpioa.pa11.into_alternate(),
            gpioa.pa12.into_alternate(),
            ccdr.peripheral.USB2OTG,
            &ccdr.clocks,
        );

        let usb_bus = Arc::new(AtomicPtr::new(&mut UsbBus::new(usb, unsafe {
            &mut EP_MEMORY
        })));

        let serial =
            usbd_serial::SerialPort::new(unsafe { usb_bus.load(Relaxed).as_ref().unwrap() });

        let usb_dev = UsbDeviceBuilder::new(
            unsafe { usb_bus.load(Relaxed).as_ref().unwrap() },
            UsbVidPid(0x16c0, 0x27dd),
        )
        .manufacturer("Fake company")
        .product("Serial port")
        .serial_number("TEST PORT 2")
        .device_class(usbd_serial::USB_CLASS_CDC)
        .build();

        let mut timer = dp
            .TIM2
            .tick_timer(1.MHz(), ccdr.peripheral.TIM2, &ccdr.clocks);
        timer.listen(timer::Event::TimeOut);

        cortex_m::interrupt::free(|cs| {
            TIMER.borrow(cs).replace(Some(timer));
        });

        unsafe {
            cp.NVIC.set_priority(pac::interrupt::TIM2, 4);
            pac::NVIC::unmask(pac::interrupt::TIM2);
        }

        Self {
            led_blue: AtomicPtr::new(&mut led_blue),
            led_green: AtomicPtr::new(&mut led_green),
            telem1: AtomicPtr::new(&mut telem1),
            usb: {
                freertos_rust::Mutex::new(USB {
                    serial,
                    device: usb_dev,
                })
                .unwrap()
            },
        }
    }

    pub fn usb_print(&self, args: fmt::Arguments) {
        match self
            .usb
            .borrow()
            .lock(freertos_rust::Duration::ms(100))
            .as_mut()
        {
            Ok(mg_usb) => mg_usb.print(args),
            Err(_) => (),
        }
    }

    pub fn usb_poll(&self) -> bool {
        match self
            .usb
            .borrow()
            .lock(freertos_rust::Duration::ms(100))
            .as_mut()
        {
            Ok(mg_usb) => {
                mg_usb.poll();
                mg_usb.read();
                match mg_usb.device.state() {
                    UsbDeviceState::Default => false,
                    UsbDeviceState::Addressed => false,
                    UsbDeviceState::Configured => true,
                    UsbDeviceState::Suspend => false,
                }
            }
            Err(_) => false,
        }
    }
}

#[interrupt]
fn TIM2() {
    match unsafe { TIM2_CALLBACK.load(SeqCst).as_ref() } {
        Some(cb) => cb(),
        None => (),
    }

    OVERFLOWS.fetch_add(1, SeqCst);
    cortex_m::interrupt::free(|cs| {
        let mut rc = TIMER.borrow(cs).borrow_mut();
        let timer = rc.as_mut().unwrap();
        timer.clear_irq();
    })
}

pub fn timestamp() -> u64 {
    let overflows = OVERFLOWS.load(SeqCst) as u64;
    let ctr = cortex_m::interrupt::free(|cs| {
        let rc = TIMER.borrow(cs).borrow();
        let timer = rc.as_ref().unwrap();
        timer.counter() as u64
    });
    100 * ((overflows << 16) + ctr)
}

lazy_static! {
    pub static ref HAL: HALDATA = HALDATA::new();
}
