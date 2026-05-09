use clap::Parser;
use inquire::{Confirm, CustomType, Select, Text};
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Overwrite existing files if they already exist
    #[arg(long, short)]
    force: bool,
}

#[derive(Debug)]
struct MotorConfig {
    field_base: String,
    constant_base: String,
    can_id: i32,
    inverted: bool,
}

#[derive(Debug)]
struct GeneratorConfig {
    subsystem: String,
    project_root: PathBuf,
    subsystems_path: PathBuf,
    package_prefix: String,
    neutral_mode: String,
    motors: Vec<MotorConfig>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut config = prompt_config()?;
    normalize_motor_identifiers(&mut config.motors);

    let package = format!("{}.{}", config.package_prefix, config.subsystem);
    let subsystem_dir = config
        .project_root
        .join(&config.subsystems_path)
        .join(&config.subsystem);

    fs::create_dir_all(&subsystem_dir)?;

    let io_interface = format!("{}IO", config.subsystem);
    let io_impl = format!("{}IOTalonFX", config.subsystem);
    let io_inputs = format!("{}IOInputs", config.subsystem);
    let constants_class = format!("{}Constants", config.subsystem);

    let files = vec![
        (
            subsystem_dir.join(format!("{constants_class}.java")),
            render_constants(&package, &constants_class, &config.motors),
        ),
        (
            subsystem_dir.join(format!("{io_interface}.java")),
            render_io_interface(
                &package,
                &io_interface,
                &io_inputs,
                &constants_class,
                &config.motors,
            ),
        ),
        (
            subsystem_dir.join(format!("{io_impl}.java")),
            render_io_impl(
                &package,
                &io_impl,
                &io_interface,
                &io_inputs,
                &constants_class,
                &config.neutral_mode,
                &config.motors,
            ),
        ),
        (
            subsystem_dir.join(format!("{}.java", config.subsystem)),
            render_subsystem(&package, &config.subsystem, &io_interface, &config.motors),
        ),
    ];

    for (path, contents) in files {
        write_file(&path, &contents, args.force)?;
        println!("Created {}", path.display());
    }

    Ok(())
}

fn prompt_config() -> Result<GeneratorConfig, Box<dyn std::error::Error>> {
    let project_root = Text::new("WPILib project root")
        .with_default(".")
        .prompt()?;
    let subsystems_path = Text::new("Subsystems path (relative to project root)")
        .with_default("src/main/java/frc/robot/subsystems")
        .prompt()?;
    let package_prefix = Text::new("Java package")
        .with_default("frc.robot.subsystems")
        .prompt()?;
    let subsystem_name = Text::new("Subsystem name")
        .with_help_message("Used for class name and package segment")
        .prompt()?;
    let subsystem = to_pascal_case(&subsystem_name);

    let motor_count = CustomType::<usize>::new("Number of TalonFX motors")
        .with_default(1)
        .with_error_message("Enter an int greater than zero")
        .prompt()?;
    if motor_count == 0 {
        return Err("motor count must be at least 1".into());
    }

    let neutral_mode = Select::new("Neutral mode for generated motors", vec!["Brake", "Coast"])
        .prompt()?
        .to_string();

    let mut motors = Vec::with_capacity(motor_count);
    for i in 0..motor_count {
        let default_name = format!("motor{}", i + 1);
        let name = Text::new(&format!("Motor {} name", i + 1))
            .with_default(&default_name)
            .with_help_message("Examples: left, right, roller, feeder")
            .prompt()?;
        let can_id = CustomType::<i32>::new(&format!("Motor {} CAN ID", i + 1))
            .with_error_message("CAN ID must be an integer from 0 to 62")
            .with_validator(|value: &i32| {
                if (0..=62).contains(value) {
                    Ok(inquire::validator::Validation::Valid)
                } else {
                    Ok(inquire::validator::Validation::Invalid(
                        "CAN ID must be between 0 and 62".into(),
                    ))
                }
            })
            .prompt()?;
        let inverted = Confirm::new(&format!("Is '{}' inverted?", name))
            .with_default(false)
            .prompt()?;

        motors.push(MotorConfig {
            field_base: to_lower_camel_case(&name),
            constant_base: to_upper_snake_case(&name),
            can_id,
            inverted,
        });
    }

    Ok(GeneratorConfig {
        subsystem,
        project_root: PathBuf::from(project_root),
        subsystems_path: PathBuf::from(subsystems_path),
        package_prefix,
        neutral_mode,
        motors,
    })
}

fn normalize_motor_identifiers(motors: &mut [MotorConfig]) {
    let mut seen_fields = HashSet::new();
    let mut seen_constants = HashSet::new();

    for (index, motor) in motors.iter_mut().enumerate() {
        let base_field = if motor.field_base.is_empty() {
            format!("motor{}", index + 1)
        } else {
            motor.field_base.clone()
        };

        let mut unique_field = base_field.clone();
        let mut n = 2;
        while !seen_fields.insert(unique_field.clone()) {
            unique_field = format!("{base_field}{n}");
            n += 1;
        }
        motor.field_base = unique_field;

        let base_constant = if motor.constant_base.is_empty() {
            format!("MOTOR_{}", index + 1)
        } else {
            motor.constant_base.clone()
        };

        let mut unique_constant = base_constant.clone();
        let mut m = 2;
        while !seen_constants.insert(unique_constant.clone()) {
            unique_constant = format!("{base_constant}_{m}");
            m += 1;
        }
        motor.constant_base = unique_constant;
    }
}

fn write_file(path: &Path, contents: &str, force: bool) -> io::Result<()> {
    if path.exists() && !force {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists. Re-run with --force to overwrite.",
                path.display()
            ),
        ));
    }

    fs::write(path, contents)
}

fn to_pascal_case(name: &str) -> String {
    let mut out = String::new();
    for part in name.split(|c: char| !c.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            for c in chars {
                out.push(c.to_ascii_lowercase());
            }
        }
    }

    if out.is_empty() {
        "Subsystem".to_string()
    } else if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("Subsystem{out}")
    } else {
        out
    }
}

fn to_lower_camel_case(name: &str) -> String {
    let pascal = to_pascal_case(name);
    if pascal.is_empty() {
        return "motor".to_string();
    }
    let mut chars = pascal.chars();
    let first = chars.next().unwrap_or('m').to_ascii_lowercase();
    let mut out = String::new();
    out.push(first);
    out.extend(chars);
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("motor{out}")
    } else {
        out
    }
}

fn to_upper_snake_case(name: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
            last_was_sep = false;
        } else if !last_was_sep && !out.is_empty() {
            out.push('_');
            last_was_sep = true;
        }
    }

    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "MOTOR".to_string()
    } else if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("MOTOR_{out}")
    } else {
        out
    }
}

fn upper_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => {
            let mut out = String::new();
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
            out
        }
        None => String::new(),
    }
}

fn render_constants(package: &str, constants_class: &str, motors: &[MotorConfig]) -> String {
    let mut motor_constants = String::new();
    for motor in motors {
        motor_constants.push_str(&format!(
            "  public static final int {}_MOTOR_ID = {};\n",
            motor.constant_base, motor.can_id
        ));
    }

    format!(
        "package {package};

public class {constants_class} {{

{motor_constants}}}
"
    )
}

fn render_io_interface(
    package: &str,
    io_interface: &str,
    io_inputs: &str,
    _constants_class: &str,
    motors: &[MotorConfig],
) -> String {
    let mut input_fields = String::new();
    for motor in motors {
        let f = &motor.field_base;
        input_fields.push_str(&format!("    public double {f}PositionRot = 0.0;\n"));
        input_fields.push_str(&format!("    public double {f}VelocityRps = 0.0;\n"));
        input_fields.push_str(&format!("    public double {f}StatorCurrentAmps = 0.0;\n"));
        input_fields.push_str(&format!("    public double {f}SupplyCurrentAmps = 0.0;\n"));
        input_fields.push_str(&format!("    public double {f}AppliedVolts = 0.0;\n"));
    }

    let mut speed_methods = String::new();
    let mut stop_calls = String::new();
    for motor in motors {
        let method_suffix = upper_first(&motor.field_base);
        speed_methods.push_str(&format!(
            "  public void set{method_suffix}SpeedRaw(double speed);\n\n"
        ));
        speed_methods.push_str(&format!(
            "  public void set{method_suffix}Control(ControlRequest request);\n\n"
        ));
        stop_calls.push_str(&format!("    set{method_suffix}SpeedRaw(0.0);\n"));
    }

    format!(
        "package {package};

import org.littletonrobotics.junction.AutoLog;
import com.ctre.phoenix6.controls.ControlRequest;

public interface {io_interface} {{
  @AutoLog
  public static class {io_inputs} {{
{input_fields}  }}

  public void updateInputs({io_inputs} inputs);

{speed_methods}  public default void stop() {{
{stop_calls}  }}

  public default void close() {{}}
}}
"
    )
}

fn render_io_impl(
    package: &str,
    io_impl: &str,
    io_interface: &str,
    io_inputs: &str,
    constants_class: &str,
    neutral_mode: &str,
    motors: &[MotorConfig],
) -> String {
    let mut motor_fields = String::new();
    for motor in motors {
        motor_fields.push_str(&format!(
            "  private final TalonFX {}Motor = new TalonFX({constants_class}.{}_MOTOR_ID, Constants.CANIVORE_SUB);\n",
            motor.field_base, motor.constant_base
        ));
    }

    let mut config_calls = String::new();
    for motor in motors {
        let inverted = if motor.inverted {
            "InvertedValue.Clockwise_Positive"
        } else {
            "InvertedValue.CounterClockwise_Positive"
        };
        let neutral = if neutral_mode == "Brake" {
            "NeutralModeValue.Brake"
        } else {
            "NeutralModeValue.Coast"
        };
        config_calls.push_str(&format!(
            "    {}Motor.getConfigurator().apply(config);\n",
            motor.field_base
        ));
        config_calls.push_str(&format!(
            "    {}Motor.getConfigurator().apply(new MotorOutputConfigs().withInverted({inverted}).withNeutralMode({neutral}));\n",
            motor.field_base
        ));
    }

    let mut input_assignments = String::new();
    for motor in motors {
        let f = &motor.field_base;
        input_assignments.push_str(&format!(
            "    inputs.{f}PositionRot = {f}Motor.getPosition().getValueAsDouble();\n"
        ));
        input_assignments.push_str(&format!(
            "    inputs.{f}VelocityRps = {f}Motor.getVelocity().getValueAsDouble();\n"
        ));
        input_assignments.push_str(&format!(
            "    inputs.{f}StatorCurrentAmps = {f}Motor.getStatorCurrent().getValueAsDouble();\n"
        ));
        input_assignments.push_str(&format!(
            "    inputs.{f}SupplyCurrentAmps = {f}Motor.getSupplyCurrent().getValueAsDouble();\n"
        ));
        input_assignments.push_str(&format!(
            "    inputs.{f}AppliedVolts = {f}Motor.getMotorVoltage().getValueAsDouble();\n"
        ));
    }

    let mut raw_speed_methods = String::new();
    for motor in motors {
        let method_suffix = upper_first(&motor.field_base);
        raw_speed_methods.push_str(&format!(
            "  @Override\n  public void set{method_suffix}SpeedRaw(double speed) {{\n    {}Motor.set(speed);\n  }}\n\n",
            motor.field_base
        ));
        raw_speed_methods.push_str(&format!(
            "  @Override\n  public void set{method_suffix}Control(ControlRequest request) {{\n    {}Motor.setControl(request);\n  }}\n\n",
            motor.field_base
        ));
    }

    let mut close_calls = String::new();
    for motor in motors {
        close_calls.push_str(&format!("    {}Motor.close();\n", motor.field_base));
    }

    format!(
        "package {package};

import com.ctre.phoenix6.configs.MotorOutputConfigs;
import com.ctre.phoenix6.configs.TalonFXConfiguration;
import com.ctre.phoenix6.controls.ControlRequest;
import com.ctre.phoenix6.hardware.TalonFX;
import com.ctre.phoenix6.signals.InvertedValue;
import com.ctre.phoenix6.signals.NeutralModeValue;
import frc.robot.constants.Constants;

public class {io_impl} implements {io_interface} {{
{motor_fields}
  public {io_impl}() {{
    TalonFXConfiguration config = new TalonFXConfiguration();
    // TODO: tune PID, current limits, and motion magic limits
{config_calls}  }}

  @Override
  public void updateInputs({io_inputs} inputs) {{
{input_assignments}  }}

{raw_speed_methods}  @Override
  public void close() {{
{close_calls}  }}
}}
"
    )
}

fn render_subsystem(
    package: &str,
    subsystem: &str,
    io_interface: &str,
    motors: &[MotorConfig],
) -> String {
    let io_inputs_auto = format!("{subsystem}IOInputsAutoLogged");
    let mut methods = String::new();
    for motor in motors {
        let method_suffix = upper_first(&motor.field_base);
        methods.push_str(&format!(
            "  public void set{method_suffix}SpeedRaw(double speed) {{\n    io.set{method_suffix}SpeedRaw(speed);\n  }}\n\n"
        ));
    }
    format!(
        "package {package};

import org.littletonrobotics.junction.Logger;

import edu.wpi.first.wpilibj2.command.SubsystemBase;

public class {subsystem} extends SubsystemBase {{
  private final {io_interface} io;
  private final {io_inputs_auto} inputs = new {io_inputs_auto}();

  public {subsystem}({io_interface} io) {{
    this.io = io;
  }}

  @Override
  public void periodic() {{
    io.updateInputs(inputs);
    Logger.processInputs(\"{subsystem}\", inputs);
  }}

{methods}  public void stop() {{
    io.stop();
  }}

  public void close() {{
    io.close();
  }}
}}
"
    )
}
