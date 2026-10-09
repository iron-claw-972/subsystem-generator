# Team 972 subsystem generator

run with `cargo run`, should work

Java source generation now uses **Askama** templates stored in `templates/`.

Each generated subsystem includes both a TalonFX-backed `*IOTalonFX` implementation
and a WPILib `DCMotorSim`-backed `*IOSim` implementation. The simulation models raw
percent output and telemetry. Phoenix-specific control requests are intentionally kept
out of the shared IO and subsystem APIs; hardware-specific closed-loop behavior belongs
in the TalonFX implementation.
