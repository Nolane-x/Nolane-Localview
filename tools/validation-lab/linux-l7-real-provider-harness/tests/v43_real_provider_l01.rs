#![cfg(target_os = "linux")]

use localview_linux_atspi_provider::{AtspiEndpoint, LinuxAtspiProvider};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

#[tokio::test]
#[ignore = "requires a real Ubuntu GTK/ATK/AT-SPI seed"]
async fn shipping_provider_exposes_live_l01_authorization_path() {
    let provider = LinuxAtspiProvider::connect(
        ProviderIncarnationRef::from("provider:linux-atspi:real:l01"),
        TargetIncarnationRef::from("target:linux-atspi:real:l01"),
    )
    .await
    .expect("real L01 harness requires a live AT-SPI provider connection");

    let binding = provider.bind(
        AtspiEndpoint::new(":1.100", "/org/a11y/atspi/accessible/l01-red"),
        "cut:l01:real:red",
    );

    let _ = provider.authorize_action(&binding).await;
}
