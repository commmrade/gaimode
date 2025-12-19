use std::time::Duration;

const SERVICE_NAME: &'static str = "gaimoded.service";

pub fn check_or_spin_up_daemon() -> anyhow::Result<()> {
    let conn = dbus::blocking::Connection::new_session()?;

    let timeout = Duration::from_millis(500);

    let proxy = conn.with_proxy(
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        timeout,
    );
    let (res,): (dbus::Path,) = proxy.method_call(
        "org.freedesktop.systemd1.Manager",
        "LoadUnit",
        (SERVICE_NAME,),
    )?;

    let properties_proxy = conn.with_proxy("org.freedesktop.systemd1", res, timeout);

    let (state,): (dbus::arg::Variant<String>,) = properties_proxy.method_call(
        "org.freedesktop.DBus.Properties",
        "Get",
        ("org.freedesktop.systemd1.Unit", "ActiveState"),
    )?;
    let state = state.0;

    if state != "active" && state != "activating" {
        // run
        let (_,): (dbus::Path,) = proxy.method_call(
            "org.freedesktop.systemd1.Manager",
            "StartUnit",
            (SERVICE_NAME, "replace"),
        )?;
        std::thread::sleep(Duration::from_millis(150));
    }
    Ok(())
}
