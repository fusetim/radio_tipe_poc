use esp_idf_sys; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported

use anyhow::{anyhow, Result};
use esp_idf_hal::delay;
use esp_idf_hal::prelude::*;
//use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use esp_idf_hal::gpio::{
    AnyIOPin, InputPin as GpioInputPin, OutputPin as GpioOutputPin, PinDriver,
};
use esp_idf_hal::spi::config::Config as SpiConfig;
use esp_idf_hal::spi::{self, Dma, SpiDeviceDriver, SpiDriver, SPI2};
use esp_idf_hal::units::{KiloHertz, MegaHertz};

use embedded_hal::blocking::delay::DelayMs;
use embedded_hal::blocking::spi::{Transactional, Transfer, Write};
use embedded_hal::digital::v2::{InputPin, OutputPin};

use radio::{Receive, Transmit};
use radio_sx127x::device::lora::{LoRaChannel, SpreadingFactor};
use radio_sx127x::device::{Channel, Config, PaConfig, PaSelect};
use radio_sx127x::Sx127xSpi;

use radio_tipe_poc::radio::LoRaRadio;

//use esp_backtrace as _;

use core::fmt::Debug;

mod echo_client;
mod echo_server;

const LORA_FREQUENCIES: [KiloHertz; 5] = [
    KiloHertz(869525),
    KiloHertz(867700),
    KiloHertz(867500),
    KiloHertz(867300),
    KiloHertz(867100),
]; // EU-868MHz band
const LORA_SPI_FREQUENCY: MegaHertz = MegaHertz(1); // MHz


fn main() -> Result<()> {
    println!("Hello, world!");

    esp_idf_logger::init().unwrap();
    log::info!("Log stuff={} and={}", 1, "hi");

    let peripherals = Peripherals::take().unwrap();

    let pins = peripherals.pins;

    /* SPI Pinout
     * SCK :        05
     * MOSI :       18
     * MISO :       19
     * NSS / nCS:   17
     * RESET:       16
     */
    let driver = SpiDriver::new::<SPI2>(
        peripherals.spi2,
        pins.gpio5,
        pins.gpio18,
        Some(pins.gpio19),
        Dma::Disabled,
    )?;
    let config = SpiConfig::new().baudrate(LORA_SPI_FREQUENCY.into());
    let mut device = SpiDeviceDriver::new(&driver, None as Option<AnyIOPin>, &config)?;
    let lora = setup_lora(
        device,
        PinDriver::input_output(pins.gpio17)?.into_output()?,
        PinDriver::input_output(pins.gpio16)?.into_input_output()?,
    )?;

    println!("LoRa radio is ready.");

    // Init TIPE PoC Client
    let delay_params = radio_tipe_poc::radio::DelayParams {
        duty_cycle: 0.01,
        min_delay: 30_000_000,      // 10s
        poll_delay: 250_000,        // 250ms
        duty_interval: 120_000_000, // 2min
    };
    let channels: Vec<radio_tipe_poc::radio::Channel<Channel>> = LORA_FREQUENCIES
        .into_iter()
        .map(|freq| {
            let radio_channel = Channel::LoRa(LoRaChannel {
                freq: freq.into(),
                sf: SpreadingFactor::Sf9,
                ..Default::default()
            });
            radio_tipe_poc::radio::Channel {
                radio_channel,
                delay: delay_params.clone(),
            }
        })
        .collect();

    let atpc = radio_tipe_poc::atpc::TestingATPC::new(vec![10, 8, 6, 4, 2]);

    let device = LoRaRadio::new(lora, &channels, atpc, -100, None, None, 0b0101_0011);
    let mut handler = echo_client::EchoClient::new(
        device,
        vec![
            "HELO1",
            "HELO2",
            "Enchante de pouvoir communiquer avec vous!",
        ]
        .into_iter()
        .map(|s| s.as_bytes().to_owned())
        .collect(),
    );

    //let device = LoRaRadio::new(lora, &channels, atpc, -100, None, None, 0b0101_0010);
    //let mut handler = echo_server::EchoServer::new(device);
    handler
        .spawn()
        .map_err(|err| anyhow!("Handler error!\ncause: {:?}", err))?;

    println!("Stopping!");

    Ok(())
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
