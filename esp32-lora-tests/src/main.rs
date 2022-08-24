use esp_idf_sys; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported

use anyhow::{ Result, anyhow };
use esp_idf_hal::prelude::*;
use esp_idf_hal::delay;
//use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use esp_idf_hal::gpio::{InputPin as GpioInputPin, OutputPin as GpioOutputPin};
use esp_idf_hal::spi::{self, Spi};
use esp_idf_hal::units::{MegaHertz,KiloHertz};

use embedded_hal::digital::v2::{InputPin, OutputPin};
use embedded_hal::blocking::spi::{Transfer, Write, Transactional};
use embedded_hal::blocking::delay::DelayMs;

use radio_sx127x::Sx127xSpi;
use radio_sx127x::device::{Config, Channel, PaSelect, PaConfig};
use radio_sx127x::device::lora::{LoRaChannel, SpreadingFactor};
use radio::{Receive,Transmit};

use core::fmt::Debug;

const LORA_FREQUENCY: KiloHertz = KiloHertz(869525); // EU-868MHz band
const LORA_SPI_FREQUENCY : MegaHertz = MegaHertz(1); // MHz
const DEVICE_NAME : &'static str = "esp32-u1";
type LoraSpi = spi::SPI2;

fn main() -> Result<()> {
    println!("Hello, world!");

    #[allow(unused)]
    let peripherals = Peripherals::take().unwrap();

    #[allow(unused)]
    let pins = peripherals.pins;

    // Should use the associated LORA_SPI.
    let spi = setup_lora_spi(peripherals.spi2, pins.gpio19, pins.gpio18, pins.gpio5)?;
    let mut lora = setup_lora(spi, pins.gpio17.into_output()?, pins.gpio16.into_input_output()?)?;

    println!("LoRa radio is ready.");

    let helo_packet = format!("{}>HELO", DEVICE_NAME);

    lora.start_receive().map_err(|_| {anyhow!("Failed to start Receive Continuous Mode!")})?;
    let mut stop = 0;
    while stop < 6*10 {
        if let Ok(true) = lora.check_receive(false) {
            let mut packet = vec![0u8;256];
            let (rx_size, pk_info) = lora.get_received(&mut packet).map_err(|_| {anyhow!("Read packet failed!")})?;
            println!("Received (size: {}): ", rx_size);
            println!("{:02X?}", &packet[..rx_size]);
        } else {
            println!("No packet received this last 10s.");
        }
        println!("Sending...");
        lora.start_transmit(helo_packet.as_bytes()).map_err(|_| {anyhow!("Transmission packet failed!")})?;
        while !lora.check_transmit().map_err(|err| {anyhow!("Failed to check transmission status\ncause: {:?}", err)})? {
           lora.delay_ms(200);
            println!("Still sending...");
        }
        stop+=1;
        println!("Start receiving..");
        lora.start_receive().map_err(|_| {anyhow!("Failed to start Receive Continuous Mode!")})?;
        lora.delay_ms(9800);
    }

    println!("Stopping!");

    Ok(())
}

fn setup_lora_spi<
    Miso: GpioOutputPin, 
    Mosi: GpioInputPin + GpioOutputPin, 
    Sck: GpioOutputPin
    >(spi_device: LoraSpi,  mosi: Mosi, miso: Miso, sck: Sck) -> Result<spi::Master<LoraSpi,Sck, Miso, Mosi>> {
    let config = <spi::config::Config as Default>::default()
        .baudrate(LORA_SPI_FREQUENCY.into());
    let spi = spi::Master::<LoraSpi,_,_,_,_>::new(
        spi_device,
        spi::Pins {
            sclk: sck,
            sdo: miso,
            sdi:Some(mosi),
            cs: None
        },
        config,
    )?;

    Ok(spi)
}

fn setup_lora<
    E : Debug + 'static,
    E2: Debug + 'static,
    S: Transfer<u8, Error = E> + Write<u8, Error=E> + Transactional<u8, Error=E>,
    Nss: OutputPin<Error = E2>,
    Reset: InputPin<Error = E2> + OutputPin<Error = E2>
>(spi: S, nss: Nss, reset: Reset) -> Result<Sx127xSpi<S,Nss,Reset,delay::Ets>> {
    //let base = Base{spi, nss, reset, delay::Ets};
    //let mut lora = radio_sx127x::Sx127x<
    //    Base<S, Nss, Reset, EtsDelay>
    //>::new(base, config).map_err(|err| {anyhow!("Lora radio setup failed!\ncause: {:?}", err)})?;
    //lora.clear_irq().map_err(|_| {anyhow!("IRQ clear failed!")})?;
    //lora.set_tx_power(10, 0).map_err(|_| {anyhow!("TX power setup failed!")})?; // Default 10dBm / 10mW on normal amplification circuit (0-14dBm circuit)
    //let _ = lora.get_interrupts(true).map_err(|_| {anyhow!("IRQ clear failed!")})?;
    //lora.set_power(10);
    let channel = Channel::LoRa(LoRaChannel {
        freq: LORA_FREQUENCY.into(),
        sf: SpreadingFactor::Sf9,
        ..Default::default()
    });
    let pa_config = PaConfig {
        power: 5,
        output: PaSelect::Boost
    };
    let config = Config {
        channel,
        pa_config,
        ..Default::default()
    };
    let lora = Sx127xSpi::spi(spi, nss, reset, delay::Ets, &config).map_err(|err| anyhow!("Lora radio setup failed!\ncause: {:?}", err))?;

    Ok(lora)
}