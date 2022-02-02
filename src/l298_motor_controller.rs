use crate::OutputPin;
use crate::PwmPin;
use anyhow::Result;

pub trait L298MotorControllerI {
    fn set_motors_speed_and_direction(&mut self, vector1: f32, vector2: f32) -> Result<()>;
}

struct Motor<IN1: OutputPin, IN2: OutputPin, EN: PwmPin> {
    in1_pin: IN1,
    in2_pin: IN2,
    en_pin: EN,
    pwm_max_duty: f32,
}

impl<IN1: OutputPin, IN2: OutputPin, EN: PwmPin> Motor<IN1, IN2, EN> {
    fn new(in1_pin: IN1, in2_pin: IN2, en_pin: EN) -> Result<Self> {
        let pwm_max_duty = match en_pin.get_max_duty() {
            Ok(v) => v as f32,
            Err(e) => anyhow::bail!("Could not get PWM pin max duty cycle: {:?}", e),
        };
        Ok(Self {
            in1_pin,
            in2_pin,
            en_pin,
            pwm_max_duty,
        })
    }

    pub fn set_direction_and_speed(&mut self, vector: f32) -> Result<()> {
        if vector == 0. {
            self.in1_pin.set_low()?;
            self.in2_pin.set_low()?;
        } else if vector > 0. {
            self.in1_pin.set_high()?;
            self.in2_pin.set_low()?;
        } else {
            self.in1_pin.set_low()?;
            self.in2_pin.set_high()?;
        }
        let duty = (self.pwm_max_duty * vector.abs()).round() as esp_idf_hal::gpio::PwmDuty;
        if let Err(e) = self.en_pin.set_duty(duty) {
            anyhow::bail!("Error setting PWM pin duty cycle: {:?}", e);
        }
        Ok(())
    }
}

pub struct L298MotorController<
    IN1: OutputPin,
    IN2: OutputPin,
    IN3: OutputPin,
    IN4: OutputPin,
    ENA: PwmPin,
    ENB: PwmPin,
> {
    motor1: Motor<IN1, IN2, ENA>,
    motor2: Motor<IN3, IN4, ENB>,
}

impl<IN1: OutputPin, IN2: OutputPin, IN3: OutputPin, IN4: OutputPin, ENA: PwmPin, ENB: PwmPin>
    L298MotorController<IN1, IN2, IN3, IN4, ENA, ENB>
{
    pub fn new(
        in1_pin: IN1,
        in2_pin: IN2,
        in3_pin: IN3,
        in4_pin: IN4,
        ena_pin: ENA,
        enb_pin: ENB,
    ) -> Result<Self> {
        Ok(Self {
            motor1: Motor::new(in1_pin, in2_pin, ena_pin)?,
            motor2: Motor::new(in3_pin, in4_pin, enb_pin)?,
        })
    }
}

impl<IN1: OutputPin, IN2: OutputPin, IN3: OutputPin, IN4: OutputPin, ENA: PwmPin, ENB: PwmPin>
    L298MotorControllerI for L298MotorController<IN1, IN2, IN3, IN4, ENA, ENB>
{
    fn set_motors_speed_and_direction(&mut self, vector1: f32, vector2: f32) -> Result<()> {
        self.motor1.set_direction_and_speed(vector1)?;
        self.motor2.set_direction_and_speed(vector2)?;
        Ok(())
    }
}
