
```bash
# Starting from gaimode directory
cd gaimoded
cargo build
sudo cp /target/debug/gaimoded /usr/bin/gaimoded
sudo cp gaimoded.service /etc/systemd/user/
cd ../gaimode
cargo run run <app> # it should start the daemon
# or
systemctl enable --user gaimoded
systemctl start --user gaimoded
