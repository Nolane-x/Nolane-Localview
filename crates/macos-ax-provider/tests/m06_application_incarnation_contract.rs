use localview_macos_ax_provider::{
    AxApplicationIncarnation, AxElementIdentity, AxObserverApplicationBindingProvider,
    AxObserverApplicationDecision, AxObserverApplicationRebindError, AxObserverRecreateDirective,
};

fn app(pid: i32, process_start_marker: u64) -> AxApplicationIncarnation {
    AxApplicationIncarnation::new("com.nolane.localview.m06-seed", pid, process_start_marker)
}

#[test]
fn observer_binding_is_scoped_to_exact_application_incarnation() {
    let provider = AxObserverApplicationBindingProvider::new();
    let current = app(4242, 100);
    let binding = provider.bind_current(current.clone());

    let decision = provider.validate_current(binding, &current);
    let AxObserverApplicationDecision::Current(binding) = decision else {
        panic!("exact application incarnation must preserve observer authority");
    };

    assert_eq!(binding.application_incarnation(), &current);
    assert!(binding.observer_creation_revision() > 0);
}

#[test]
fn same_pid_with_new_process_start_marker_is_reincarnation() {
    let provider = AxObserverApplicationBindingProvider::new();
    let old = app(4242, 100);
    let new = app(4242, 101);
    let binding = provider.bind_current(old.clone());
    let old_revision = binding.observer_creation_revision();

    let decision = provider.validate_current(binding, &new);
    let AxObserverApplicationDecision::ApplicationReincarnated(retired) = decision else {
        panic!("PID reuse must not preserve an observer across application reincarnation");
    };

    assert_eq!(retired.previous_application_incarnation(), &old);
    assert_eq!(retired.current_application_incarnation(), &new);
    assert_eq!(
        retired.recreate_directive(),
        AxObserverRecreateDirective::CreateObserverForCurrentApplication
    );
    assert_eq!(retired.invalidated_observer_creation_revision(), old_revision);

    let fresh = provider
        .rebind_after_reincarnation(retired, new.clone())
        .expect("same semantic application may create a fresh observer for its new incarnation");
    assert_eq!(fresh.application_incarnation(), &new);
    assert!(fresh.observer_creation_revision() > old_revision);
}

#[test]
fn relaunch_with_new_pid_requires_new_observer_lifecycle() {
    let provider = AxObserverApplicationBindingProvider::new();
    let old = app(4242, 100);
    let new = app(4343, 200);
    let binding = provider.bind_current(old);

    let decision = provider.validate_current(binding, &new);
    let AxObserverApplicationDecision::ApplicationReincarnated(retired) = decision else {
        panic!("relaunch must retire the observer bound to the old process incarnation");
    };

    let fresh = provider
        .rebind_after_reincarnation(retired, new.clone())
        .expect("relaunch of the same semantic application must permit a fresh observer");
    assert_eq!(fresh.application_incarnation(), &new);
}

#[test]
fn different_semantic_application_cannot_reuse_reincarnation_tombstone() {
    let provider = AxObserverApplicationBindingProvider::new();
    let old = app(4242, 100);
    let current = AxApplicationIncarnation::new("com.example.other-app", 4343, 200);
    let binding = provider.bind_current(old);

    let decision = provider.validate_current(binding, &current);
    let AxObserverApplicationDecision::ApplicationReincarnated(retired) = decision else {
        panic!("changed application identity must retire the old observer");
    };

    assert_eq!(
        provider.rebind_after_reincarnation(retired, current),
        Err(AxObserverApplicationRebindError::ApplicationLineageChanged)
    );
}

#[test]
fn element_identity_carries_application_incarnation_not_pid_alone() {
    let first = app(4242, 100);
    let reused_pid = app(4242, 101);

    let first_element = AxElementIdentity::new(first.clone(), "window", "target");
    let reincarnated_element = AxElementIdentity::new(reused_pid.clone(), "window", "target");

    assert_eq!(first_element.application_incarnation(), &first);
    assert_eq!(first_element.application_pid(), 4242);
    assert_ne!(first_element, reincarnated_element);
}
