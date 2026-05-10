use askama::Template;
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

#[derive(Debug)]
struct MotorTemplate {
    field_base: String,
    constant_base: String,
    method_suffix: String,
    can_id: i32,
    inverted_value: &'static str,
}

#[derive(Template)]
#[template(path = "constants.java.askama")]
struct ConstantsTemplate<'a> {
    package: &'a str,
    constants_class: &'a str,
    motors: &'a [MotorTemplate],
}

#[derive(Template)]
#[template(path = "io_interface.java.askama")]
struct IoInterfaceTemplate<'a> {
    package: &'a str,
    io_interface: &'a str,
    io_inputs: &'a str,
    motors: &'a [MotorTemplate],
}

#[derive(Template)]
#[template(path = "io_impl.java.askama")]
struct IoImplTemplate<'a> {
    package: &'a str,
    io_impl: &'a str,
    io_interface: &'a str,
    io_inputs: &'a str,
    constants_class: &'a str,
    neutral_mode_value: &'a str,
    motors: &'a [MotorTemplate],
}

#[derive(Template)]
#[template(path = "subsystem.java.askama")]
struct SubsystemTemplate<'a> {
    package: &'a str,
    subsystem: &'a str,
    io_interface: &'a str,
    io_inputs_auto: String,
    motors: &'a [MotorTemplate],
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

    let template_motors = to_template_motors(&config.motors);
    let files = vec![
        (
            subsystem_dir.join(format!("{constants_class}.java")),
            render_constants(&package, &constants_class, &template_motors)?,
        ),
        (
            subsystem_dir.join(format!("{io_interface}.java")),
            render_io_interface(&package, &io_interface, &io_inputs, &template_motors)?,
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
                &template_motors,
            )?,
        ),
        (
            subsystem_dir.join(format!("{}.java", config.subsystem)),
            render_subsystem(&package, &config.subsystem, &io_interface, &template_motors)?,
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

fn to_template_motors(motors: &[MotorConfig]) -> Vec<MotorTemplate> {
    motors
        .iter()
        .map(|motor| MotorTemplate {
            field_base: motor.field_base.clone(),
            constant_base: motor.constant_base.clone(),
            method_suffix: upper_first(&motor.field_base),
            can_id: motor.can_id,
            inverted_value: if motor.inverted {
                "InvertedValue.Clockwise_Positive"
            } else {
                "InvertedValue.CounterClockwise_Positive"
            },
        })
        .collect()
}

fn render_constants(
    package: &str,
    constants_class: &str,
    motors: &[MotorTemplate],
) -> Result<String, askama::Error> {
    ConstantsTemplate {
        package,
        constants_class,
        motors,
    }
    .render()
}

fn render_io_interface(
    package: &str,
    io_interface: &str,
    io_inputs: &str,
    motors: &[MotorTemplate],
) -> Result<String, askama::Error> {
    IoInterfaceTemplate {
        package,
        io_interface,
        io_inputs,
        motors,
    }
    .render()
}

fn render_io_impl(
    package: &str,
    io_impl: &str,
    io_interface: &str,
    io_inputs: &str,
    constants_class: &str,
    neutral_mode: &str,
    motors: &[MotorTemplate],
) -> Result<String, askama::Error> {
    let neutral_mode_value = if neutral_mode == "Brake" {
        "NeutralModeValue.Brake"
    } else {
        "NeutralModeValue.Coast"
    };
    IoImplTemplate {
        package,
        io_impl,
        io_interface,
        io_inputs,
        constants_class,
        neutral_mode_value,
        motors,
    }
    .render()
}

fn render_subsystem(
    package: &str,
    subsystem: &str,
    io_interface: &str,
    motors: &[MotorTemplate],
) -> Result<String, askama::Error> {
    SubsystemTemplate {
        package,
        subsystem,
        io_interface,
        io_inputs_auto: format!("{subsystem}IOInputsAutoLogged"),
        motors,
    }
    .render()
}
