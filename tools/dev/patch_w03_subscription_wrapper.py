from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
subscription_path = ROOT / "crates/windows-uia-provider/src/subscription.rs"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one marker, found {count}")
    return text.replace(old, new, 1)


subscription = subscription_path.read_text(encoding="utf-8")
wrapper_methods = r'''

        pub(crate) fn query_virtualized_item_on_mta(
            &self,
            attachment: &WindowsUiaAttachment,
            request: crate::WindowsUiaVirtualizedItemQueryRequest,
        ) -> Result<crate::WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaWorkerError> {
            self.inner.query_virtualized_item_on_mta(attachment, request)
        }

        pub(crate) fn realize_virtualized_item_on_mta(
            &self,
            attachment: &WindowsUiaAttachment,
            request: crate::WindowsUiaVirtualizedItemRealizeRequest,
        ) -> Result<crate::WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaWorkerError> {
            self.inner.realize_virtualized_item_on_mta(attachment, request)
        }
'''
subscription = replace_once(
    subscription,
    '        pub fn verify_set_value(\n',
    wrapper_methods + '\n        pub fn verify_set_value(\n',
    "subscription worker virtualized delegates",
)
subscription_path.write_text(subscription, encoding="utf-8")
print("W03 subscription wrapper patch applied deterministically")
