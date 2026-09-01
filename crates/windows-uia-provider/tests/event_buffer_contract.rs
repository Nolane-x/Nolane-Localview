use chrono::Utc;
use localview_native_provider::ProviderEventOrdering;
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};
use localview_windows_uia_provider::{
    WindowsUiaEventBuffer, WindowsUiaEventBufferError, WindowsUiaEventDraft, WindowsUiaEventKind,
};

fn provider() -> ProviderIncarnationRef {
    ProviderIncarnationRef::from("provider:windows-uia:mta:event-buffer")
}

fn target() -> TargetIncarnationRef {
    TargetIncarnationRef::from("target:windows:selection=event-buffer")
}

fn element_ref(provider: ProviderIncarnationRef, target: TargetIncarnationRef) -> ProviderElementRef {
    ProviderElementRef {
        provider_family: "windows_uia".into(),
        provider_incarnation_ref: provider,
        target_incarnation_ref: target,
        opaque_provider_element_id: "uia-runtime:[42,7]".into(),
        semantic_locator_hints: vec!["automation_id=save".into()],
        parent_surface_ref: Some("window:1234".into()),
        acquisition_cut_ref: "uia-event:source".into(),
        realization: ProviderElementRealization::RealizedCurrent,
        lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
    }
}

fn property_event(property_id: i32) -> WindowsUiaEventDraft {
    WindowsUiaEventDraft {
        kind: WindowsUiaEventKind::PropertyChanged { property_id },
        element_ref: Some(element_ref(provider(), target())),
    }
}

#[test]
fn event_buffer_starts_at_an_explicit_zero_sequence_baseline_and_opaque_reliability() {
    let buffer = WindowsUiaEventBuffer::new(provider(), target(), 8).unwrap();

    assert_eq!(buffer.sequence_baseline(), 0);
    assert_eq!(
        buffer.reliability_profile().ordering,
        ProviderEventOrdering::OpaqueBestEffort
    );
    assert!(!buffer.reliability_profile().global_polling_required);
    assert!(
        buffer
            .reliability_profile()
            .action_critical_properties_require_reconciliation
    );
}

#[test]
fn sequence_is_allocated_before_bounded_retention_can_drop_an_event() {
    let mut buffer = WindowsUiaEventBuffer::new(provider(), target(), 2).unwrap();

    assert_eq!(buffer.push(property_event(1)).unwrap(), 1);
    assert_eq!(buffer.push(property_event(2)).unwrap(), 2);
    assert_eq!(buffer.push(property_event(3)).unwrap(), 3);
    assert_eq!(buffer.push(property_event(4)).unwrap(), 4);

    let drained = buffer.drain(16);
    assert_eq!(drained.dropped_before_drain, 2);
    assert_eq!(
        drained
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![3, 4]
    );
    assert_eq!(drained.latest_sequence, 4);

    assert_eq!(buffer.push(property_event(5)).unwrap(), 5);
    let next = buffer.drain(16);
    assert_eq!(next.dropped_before_drain, 0);
    assert_eq!(next.events[0].sequence, 5);
}

#[test]
fn retained_events_are_stamped_with_the_buffer_lineage() {
    let mut buffer = WindowsUiaEventBuffer::new(provider(), target(), 4).unwrap();
    buffer
        .push(WindowsUiaEventDraft {
            kind: WindowsUiaEventKind::StructureChanged { change_type: 2 },
            element_ref: None,
        })
        .unwrap();

    let drained = buffer.drain(1);
    let event = &drained.events[0];
    assert_eq!(event.provider_incarnation_ref, provider());
    assert_eq!(event.target_incarnation_ref, target());
    assert_eq!(event.sequence, 1);
}

#[test]
fn event_capture_time_is_allocated_at_buffer_admission() {
    let mut buffer = WindowsUiaEventBuffer::new(provider(), target(), 4).unwrap();
    let before = Utc::now();
    buffer.push(property_event(30005)).unwrap();
    let after = Utc::now();

    let event = buffer.drain(1).events.pop().unwrap();
    assert!(event.captured_at >= before);
    assert!(event.captured_at <= after);
}

#[test]
fn event_element_from_another_incarnation_is_rejected_instead_of_relabelled() {
    let mut buffer = WindowsUiaEventBuffer::new(provider(), target(), 4).unwrap();

    let wrong_provider = WindowsUiaEventDraft {
        kind: WindowsUiaEventKind::FocusChanged,
        element_ref: Some(element_ref(
            ProviderIncarnationRef::from("provider:windows-uia:mta:other"),
            target(),
        )),
    };
    assert_eq!(
        buffer.push(wrong_provider).unwrap_err(),
        WindowsUiaEventBufferError::ProviderIncarnationMismatch
    );

    let wrong_target = WindowsUiaEventDraft {
        kind: WindowsUiaEventKind::FocusChanged,
        element_ref: Some(element_ref(
            provider(),
            TargetIncarnationRef::from("target:windows:selection=other"),
        )),
    };
    assert_eq!(
        buffer.push(wrong_target).unwrap_err(),
        WindowsUiaEventBufferError::TargetIncarnationMismatch
    );

    assert_eq!(buffer.sequence_baseline(), 0);
    assert!(buffer.drain(8).events.is_empty());
}

#[test]
fn zero_capacity_is_rejected_fail_closed() {
    assert_eq!(
        WindowsUiaEventBuffer::new(provider(), target(), 0).unwrap_err(),
        WindowsUiaEventBufferError::InvalidCapacity
    );
}
