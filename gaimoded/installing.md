
```bash
# Starting from gaimode directory
cd gaimoded
cargo build
sudo cp /target/debug/gaimoded /usr/bin/gaimoded
sudo cp gaimoded.service /etc/systemd/system/
cd ../gaimode
sudo systemctl enable --now gaimoded
cargo run run <app> 
# setup polkit
sudo cp 50-gaimoded.rules /etc/polkit-1/rules.d/50-gaimoded.rules
sudo groupadd gaimode
sudo usermod -aG gaimode $USER
```
