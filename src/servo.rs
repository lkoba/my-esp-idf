use crate::PwmPinWithMicros;
use anyhow::Result;

pub struct Angle(pub f32);

pub trait ServoI {
    fn enable(&mut self) -> Result<()>;
    fn disable(&mut self) -> Result<()>;
    fn set_angle(&mut self, angle: Angle) -> Result<()>;
}

pub struct Servo<P: PwmPinWithMicros> {
    pin: P,
    min_pulsewidth: f32,
    max_pulsewidth: f32,
    max_degrees: f32,
}

impl<P: PwmPinWithMicros> Servo<P> {
    pub fn new(
        pin: P,
        max_degrees: Angle,
        min_pulsewidth: std::time::Duration,
        max_pulsewidth: std::time::Duration,
    ) -> Self {
        Servo {
            pin,
            min_pulsewidth: min_pulsewidth.as_micros() as u32 as f32,
            max_pulsewidth: max_pulsewidth.as_micros() as u32 as f32,
            max_degrees: max_degrees.0,
        }
    }
}

impl<P: PwmPinWithMicros> ServoI for Servo<P> {
    #[inline]
    fn set_angle(&mut self, angle: Angle) -> Result<()> {
        let duty = {
            let min_input = 0.;
            let max_input = self.max_degrees;
            let min_output = self.min_pulsewidth;
            let max_output = self.max_pulsewidth;
            let angle = angle.0;
            (min_output
                + (angle.clamp(min_input, max_input) - min_input) / (max_input - min_input)
                    * (max_output - min_output))
                .round() as u32
        };
        self.pin.set_duty_micros(duty)?;
        Ok(())
    }

    fn enable(&mut self) -> Result<()> {
        self.pin.enable().unwrap();
        Ok(())
    }

    fn disable(&mut self) -> Result<()> {
        self.pin.disable().unwrap();
        Ok(())
    }
}
