#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Delay, Duration, Timer};
use esp_backtrace as _;
use esp_hal::{clock::ClockControl, dma_descriptors, entry, gpio::{Io, Level, Output}, peripherals::Peripherals, prelude::*, spi::{
    master::{prelude::*, Spi},
    SpiMode,
}, system::SystemControl, timer::timg::TimerGroup};
use esp_hal::dma::{Dma, DmaPriority};
use esp_hal::gpio::{Input, Pull};
use esp_hal::timer::{ErasedTimer, OneShotTimer};
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::lorawan_radio::LorawanRadio;
use lora_phy::sx126x::{self, Sx1262, Sx126x};
use lora_phy::LoRa;
use lora_phy::sx126x::TcxoCtrlVoltage::Ctrl1V7;
use lorawan_device::async_device::{region, Device, EmbassyTimer, JoinMode};
use lorawan_device::default_crypto::DefaultFactory as Crypto;
use lorawan_device::{AppEui, AppKey, DevEui};

const LORAWAN_REGION: region::Region = region::Region::EU868;
const MAX_TX_POWER: u8 = 14;

// Load optional override values for EUIs and APPKEY generated by build.rs from
// environment values
include!(concat!(env!("OUT_DIR"), "/lorawan_keys.rs"));

// Fallback values
const DEFAULT_DEVEUI: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
const DEFAULT_APPEUI: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
const DEFAULT_APPKEY: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[embassy_executor::task]
async fn run() {
    loop {
        esp_println::println!("Hello world from embassy using esp-hal-async!");
        Timer::after(Duration::from_millis(1_000)).await;
    }
}

//#[esp_hal_embassy::main]
#[esp_hal_procmacros::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    esp_println::println!("Init!");
    let peripherals = Peripherals::take();
    let system = SystemControl::new(peripherals.SYSTEM);
    let clocks = ClockControl::boot_defaults(system.clock_control).freeze();

    let timg0 = TimerGroup::new(peripherals.TIMG0, &clocks, None);
    let timer0: ErasedTimer = timg0.timer0.into();
    let timers = [OneShotTimer::new(timer0)];
    let timers = mk_static!([OneShotTimer<ErasedTimer>; 1], timers);
    esp_hal_embassy::init(&clocks, timers);

    let io = Io::new(peripherals.GPIO, peripherals.IO_MUX);
    let mut led = Output::new(io.pins.gpio35, Level::High);

    let nss = Output::new(io.pins.gpio8, Level::High);
    let reset = Output::new(io.pins.gpio12, Level::High);
    let dio1 = Input::new(io.pins.gpio14, Pull::None);
    let busy = Input::new(io.pins.gpio13, Pull::None);

    let miso = io.pins.gpio11;
    let mosi = io.pins.gpio10;
    let sck = io.pins.gpio9;

    let dma = Dma::new(peripherals.DMA);

    let dma_channel = dma.channel0;

    let (descriptors, rx_descriptors) = dma_descriptors!(32000);

    let mut spi = Spi::new(peripherals.SPI2, 100.kHz(), SpiMode::Mode0, &clocks)
        .with_pins(Some(sck), Some(mosi), Some(miso), None)
        .with_dma(
            dma_channel.configure_for_async(false, DmaPriority::Priority0),
            descriptors,
            rx_descriptors,
        );

    let config = sx126x::Config {
        chip: Sx1262,
        tcxo_ctrl: Some(Ctrl1V7),
        use_dcdc: true,
        rx_boost: false,
    };
    let iv = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None).unwrap();
    let lora = LoRa::new(Sx126x::new(spi, iv, config), true, Delay).await.unwrap();

    let radio: LorawanRadio<_, _, MAX_TX_POWER> = lora.into();
    let region: region::Configuration = region::Configuration::new(LORAWAN_REGION);
    let mut device: Device<_, Crypto, _, _> = Device::new_with_seed(region, radio, EmbassyTimer::new(), 42u64);

    esp_println::println!("Joining LoRaWAN network");

    let resp = device
        .join(&JoinMode::OTAA {
            deveui: DevEui::from(DEVEUI.unwrap_or(DEFAULT_DEVEUI)),
            appeui: AppEui::from(APPEUI.unwrap_or(DEFAULT_APPEUI)),
            appkey: AppKey::from(APPKEY.unwrap_or(DEFAULT_APPKEY)),
        })
        .await
        .unwrap();

    esp_println::println!("LoRaWAN network joined: {:?}", resp);

    spawner.spawn(run()).ok();

    loop {
        esp_println::println!("Bing!");
        led.toggle();
        Timer::after(Duration::from_millis(5_000)).await;
    }
}