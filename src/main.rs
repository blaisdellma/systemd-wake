fn main() {
    if std::env::args().len() > 1 {
        let encoded_command = std::env::args().nth(1).unwrap();
        let command = systemd_wake::command::CommandConfig::decode(encoded_command);
        _ = systemd_wake::run_command(command);
    }
}
