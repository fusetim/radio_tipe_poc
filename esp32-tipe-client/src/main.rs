use esp_idf_sys; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported

use anyhow::{anyhow, Result};
use esp_idf_hal::delay;
use esp_idf_hal::prelude::*;
//use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use esp_idf_hal::gpio::{InputPin as GpioInputPin, OutputPin as GpioOutputPin};
use esp_idf_hal::spi::{self, Spi};
use esp_idf_hal::units::{KiloHertz, MegaHertz};

use embedded_hal::blocking::delay::DelayMs;
use embedded_hal::blocking::spi::{Transactional, Transfer, Write};
use embedded_hal::digital::v2::{InputPin, OutputPin};

use radio::{Receive, Transmit};
use radio_sx127x::device::lora::{LoRaChannel, SpreadingFactor};
use radio_sx127x::device::{Channel, Config, PaConfig, PaSelect};
use radio_sx127x::Sx127xSpi;

use radio_tipe_poc::device::radio::LoRaRadio;

use core::fmt::Debug;

mod echo_server;
mod echo_client;


const LORA_FREQUENCIES: [KiloHertz; 5] = [KiloHertz(869525),KiloHertz(867700),KiloHertz(867500),KiloHertz(867300),KiloHertz(867100)]; // EU-868MHz band
const LORA_SPI_FREQUENCY: MegaHertz = MegaHertz(1); // MHz
const DEVICE_NAME: &'static str = "esp32-u1";
type LoraSpi = spi::SPI2;

fn main() -> Result<()> {
    println!("Hello, world!");

    esp_idf_logger::init().unwrap();
    log::info!("Log stuff={} and={}", 1, "hi");

    #[allow(unused)]
    let peripherals = Peripherals::take().unwrap();

    #[allow(unused)]
    let pins = peripherals.pins;

    // Should use the associated LORA_SPI.
    let spi = setup_lora_spi(peripherals.spi2, pins.gpio19, pins.gpio18, pins.gpio5)?;
    let mut lora = setup_lora(
        spi,
        pins.gpio17.into_output()?,
        pins.gpio16.into_input_output()?,
    )?;

    println!("LoRa radio is ready.");

    // Init TIPE PoC Client
    let delay_params = radio_tipe_poc::device::radio::DelayParams {
        duty_cycle: 0.01,
        min_delay: 30_000_000,      // 10s
        poll_delay: 250_000,        // 250ms
        duty_interval: 120_000_000, // 2min
    };
    let channels : Vec<radio_tipe_poc::device::radio::Channel<Channel>> = LORA_FREQUENCIES.into_iter().map(|freq| {
        let radio_channel = Channel::LoRa(LoRaChannel{
            freq: freq.into(),
            sf: SpreadingFactor::Sf9,
            ..Default::default()
        });
        radio_tipe_poc::device::radio::Channel {
            radio_channel,
            delay: delay_params.clone(),
        }
    }).collect();

    let mut device = LoRaRadio::new(lora, &channels, None, None, 0b0101_0010);

    let mut handler = echo_client::EchoClient::new(device, vec!("HELO1", "HELO2", "Enchante de pouvoir communiquer avec vous!").into_iter().map(|s| s.as_bytes().to_owned()).collect());
    //let mut handler = echo_server::EchoServer::new(device);
    handler.spawn().map_err(|err| anyhow!("Handler error!\ncause: {:?}", err))?;

    println!("Stopping!");

    Ok(())
}

fn setup_lora_spi<Miso: GpioOutputPin, Mosi: GpioInputPin + GpioOutputPin, Sck: GpioOutputPin>(
    spi_device: LoraSpi,
    mosi: Mosi,
    miso: Miso,
    sck: Sck,
) -> Result<spi::Master<LoraSpi, Sck, Miso, Mosi>> {
    let config = <spi::config::Config as Default>::default().baudrate(LORA_SPI_FREQUENCY.into());
    let spi = spi::Master::<LoraSpi, _, _, _, _>::new(
        spi_device,
        spi::Pins {
            sclk: sck,
            sdo: miso,
            sdi: Some(mosi),
            cs: None,
        },
        config,
    )?;

    Ok(spi)
}

fn setup_lora<
    E: Debug + 'static,
    E2: Debug + 'static,
    S: Transfer<u8, Error = E> + Write<u8, Error = E> + Transactional<u8, Error = E>,
    Nss: OutputPin<Error = E2>,
    Reset: InputPin<Error = E2> + OutputPin<Error = E2>,
>(
    spi: S,
    nss: Nss,
    reset: Reset,
) -> Result<Sx127xSpi<S, Nss, Reset, delay::Ets>> {
    //let base = Base{spi, nss, reset, delay::Ets};
    //let mut lora = radio_sx127x::Sx127x<
    //    Base<S, Nss, Reset, EtsDelay>
    //>::new(base, config).map_err(|err| {anyhow!("Lora radio setup failed!\ncause: {:?}", err)})?;
    //lora.clear_irq().map_err(|_| {anyhow!("IRQ clear failed!")})?;
    //lora.set_tx_power(10, 0).map_err(|_| {anyhow!("TX power setup failed!")})?; // Default 10dBm / 10mW on normal amplification circuit (0-14dBm circuit)
    //let _ = lora.get_interrupts(true).map_err(|_| {anyhow!("IRQ clear failed!")})?;
    //lora.set_power(10);
    let channel = Channel::LoRa(LoRaChannel {
        freq: LORA_FREQUENCIES[0].into(),
        sf: SpreadingFactor::Sf9,
        ..Default::default()
    });
    let pa_config = PaConfig {
        power: 10,
        output: PaSelect::Boost,
    };
    let config = Config {
        channel,
        pa_config,
        ..Default::default()
    };
    let lora = Sx127xSpi::spi(spi, nss, reset, delay::Ets, &config)
        .map_err(|err| anyhow!("Lora radio setup failed!\ncause: {:?}", err))?;

    Ok(lora)
}
