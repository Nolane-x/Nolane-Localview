using System.Collections.ObjectModel;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Automation.Peers;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;

namespace LocalView.WindowsUiaEdgeSeed;

internal sealed class EdgeWindow : Window
{
    public const int VirtualItemIndex = 255;
    public const string VirtualItemName = "LocalView Virtual Item 255";
    public const string VirtualizedListAutomationId = "LocalViewW03VirtualizedItems";
    public const string VirtualizedListName = "LocalView W03 Virtualized Items";
    public const string HostileProviderAutomationId = "LocalViewW05HostileProvider";
    public const string HostileProviderName = "LocalView W05 Hostile Provider";
    public const string VerifiedInputTargetAutomationId = "LocalViewW07W09VerifiedInputTarget";

    private const byte VkShift = 0x10;
    private const uint KeyEventKeyUp = 0x0002;

    private readonly ListBox _virtualizedList;
    private readonly Button _verifiedInputTarget;
    private readonly ManualResetEventSlim _providerHangRelease = new(false);
    private Window? _foregroundThief;
    private Window? _modalBlocker;
    private int _providerHangArmed;
    private int _providerCallEntered;
    private int _verifiedInputEffectCount;
    private bool _shiftFixtureOwned;

    public EdgeWindow()
    {
        Title = "LocalView Windows UIA Edge Seed";
        Width = 520;
        Height = 320;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;

        var items = new ObservableCollection<string>(
            Enumerable.Range(0, VirtualItemIndex + 1)
                .Select(index => $"LocalView Virtual Item {index}"));

        _verifiedInputTarget = new Button
        {
            Name = "VerifiedInputTarget",
            Content = "LocalView W07/W09 verified input target",
            Height = 32,
            Margin = new Thickness(16, 8, 16, 0),
            Focusable = true,
        };
        AutomationProperties.SetAutomationId(
            _verifiedInputTarget,
            VerifiedInputTargetAutomationId);
        AutomationProperties.SetName(
            _verifiedInputTarget,
            "LocalView W07/W09 verified input target");
        _verifiedInputTarget.PreviewKeyDown += (_, eventArgs) =>
        {
            if (eventArgs.Key == Key.Space)
            {
                Interlocked.Increment(ref _verifiedInputEffectCount);
            }
        };

        _virtualizedList = new ListBox
        {
            Name = "VirtualizedItems",
            Height = 120,
            Margin = new Thickness(16),
            ItemsSource = items,
        };
        AutomationProperties.SetAutomationId(_virtualizedList, VirtualizedListAutomationId);
        AutomationProperties.SetName(_virtualizedList, VirtualizedListName);

        VirtualizingStackPanel.SetIsVirtualizing(_virtualizedList, true);
        VirtualizingStackPanel.SetVirtualizationMode(
            _virtualizedList,
            VirtualizationMode.Recycling);
        ScrollViewer.SetCanContentScroll(_virtualizedList, true);

        var hostileProvider = new HostileProviderElement(this)
        {
            Height = 24,
            Margin = new Thickness(16, 8, 16, 0),
        };
        AutomationProperties.SetAutomationId(hostileProvider, HostileProviderAutomationId);

        var content = new StackPanel();
        content.Children.Add(hostileProvider);
        content.Children.Add(_verifiedInputTarget);
        content.Children.Add(_virtualizedList);
        Content = content;

        Loaded += (_, _) =>
        {
            UpdateLayout();
            _verifiedInputTarget.UpdateLayout();
            _virtualizedList.UpdateLayout();
        };
        Closed += (_, _) =>
        {
            ReleaseProviderHang();
            CleanupVerifiedInputFixture();
        };
    }

    public bool IsVirtualItemContainerGenerated()
    {
        return _virtualizedList.ItemContainerGenerator.ContainerFromIndex(VirtualItemIndex) is not null;
    }

    public void PrepareVerifiedInputTarget()
    {
        if (_shiftFixtureOwned)
        {
            ReleaseShiftFixture();
        }
        CloseModalBlocker();
        CloseForegroundThief();
        Interlocked.Exchange(ref _verifiedInputEffectCount, 0);

        Show();
        RestoreVerifiedInputTargetForeground();
    }

    public void StealForeground()
    {
        CloseModalBlocker();
        CloseForegroundThief();
        _foregroundThief = new Window
        {
            Title = "LocalView W07 foreground thief",
            Width = 320,
            Height = 140,
            ResizeMode = ResizeMode.NoResize,
            WindowStartupLocation = WindowStartupLocation.CenterScreen,
            ShowInTaskbar = false,
            Topmost = true,
            Content = new TextBlock
            {
                Text = "LocalView W07 deterministic foreground thief",
                Margin = new Thickness(16),
                VerticalAlignment = VerticalAlignment.Center,
                HorizontalAlignment = HorizontalAlignment.Center,
            },
        };
        _foregroundThief.Show();
        _foregroundThief.Activate();
        var thiefHandle = ForegroundThiefWindowHandle();
        if (thiefHandle != 0)
        {
            _ = SetForegroundWindow((nint)thiefHandle);
        }
    }

    public void CloseForegroundThief()
    {
        if (_foregroundThief is null)
        {
            return;
        }
        var thief = _foregroundThief;
        _foregroundThief = null;
        thief.Close();
    }

    public void OpenModalBlocker()
    {
        CloseForegroundThief();
        CloseModalBlocker();
        _modalBlocker = new Window
        {
            Owner = this,
            Title = "LocalView W11 owned modal blocker",
            Width = 360,
            Height = 160,
            ResizeMode = ResizeMode.NoResize,
            WindowStartupLocation = WindowStartupLocation.CenterOwner,
            ShowInTaskbar = false,
            Topmost = true,
            Content = new TextBlock
            {
                Text = "LocalView W11 deterministic owned modal blocker",
                Margin = new Thickness(16),
                VerticalAlignment = VerticalAlignment.Center,
                HorizontalAlignment = HorizontalAlignment.Center,
            },
        };
        _modalBlocker.Show();
        _modalBlocker.Activate();

        // Register the owned window as the target's last active popup, then
        // deliberately restore target foreground/focus. This isolates W11's
        // modal-blocker fence from W07's foreground-mismatch fence.
        RestoreVerifiedInputTargetForeground();
    }

    public void CloseModalBlocker()
    {
        if (_modalBlocker is null)
        {
            return;
        }
        var modal = _modalBlocker;
        _modalBlocker = null;
        modal.Close();
        if (IsLoaded)
        {
            RestoreVerifiedInputTargetForeground();
        }
    }

    public long ModalBlockerWindowHandle()
    {
        return _modalBlocker is null
            ? 0
            : new WindowInteropHelper(_modalBlocker).Handle.ToInt64();
    }

    public long ModalBlockerOwnerWindowHandle()
    {
        return _modalBlocker is null
            ? 0
            : new WindowInteropHelper(_modalBlocker).Owner.ToInt64();
    }

    public bool IsModalBlockerOpen()
    {
        return _modalBlocker is not null && _modalBlocker.IsVisible;
    }

    public void HoldShiftFixture()
    {
        if (_shiftFixtureOwned)
        {
            return;
        }
        if (IsShiftDown())
        {
            throw new InvalidOperationException("shift_already_down");
        }

        keybd_event(VkShift, 0, 0, UIntPtr.Zero);
        _shiftFixtureOwned = true;
        if (!IsShiftDown())
        {
            ReleaseShiftFixture();
            throw new InvalidOperationException("shift_keydown_not_observed");
        }
    }

    public void ReleaseShiftFixture()
    {
        if (!_shiftFixtureOwned)
        {
            return;
        }
        keybd_event(VkShift, 0, KeyEventKeyUp, UIntPtr.Zero);
        _shiftFixtureOwned = false;
    }

    public void CleanupVerifiedInputFixture()
    {
        ReleaseShiftFixture();
        CloseModalBlocker();
        CloseForegroundThief();
    }

    public long WindowHandle()
    {
        return new WindowInteropHelper(this).Handle.ToInt64();
    }

    public long ForegroundThiefWindowHandle()
    {
        return _foregroundThief is null
            ? 0
            : new WindowInteropHelper(_foregroundThief).Handle.ToInt64();
    }

    public long ForegroundWindowHandle()
    {
        return GetForegroundWindow().ToInt64();
    }

    public bool IsTargetForeground()
    {
        var handle = WindowHandle();
        return handle != 0 && ForegroundWindowHandle() == handle;
    }

    public bool IsThiefForeground()
    {
        var handle = ForegroundThiefWindowHandle();
        return handle != 0 && ForegroundWindowHandle() == handle;
    }

    public bool IsShiftDown()
    {
        return (GetAsyncKeyState(VkShift) & 0x8000) != 0;
    }

    public int VerifiedInputEffectCount()
    {
        return Volatile.Read(ref _verifiedInputEffectCount);
    }

    public void ArmProviderHang()
    {
        _providerHangRelease.Reset();
        Interlocked.Exchange(ref _providerCallEntered, 0);
        Interlocked.Exchange(ref _providerHangArmed, 1);
    }

    public bool IsProviderHangArmed()
    {
        return Volatile.Read(ref _providerHangArmed) != 0;
    }

    public bool ProviderCallEntered()
    {
        return Volatile.Read(ref _providerCallEntered) != 0;
    }

    public void ReleaseProviderHang()
    {
        Interlocked.Exchange(ref _providerHangArmed, 0);
        _providerHangRelease.Set();
    }

    internal void WaitIfProviderHangArmed()
    {
        if (!IsProviderHangArmed())
        {
            return;
        }

        Interlocked.Exchange(ref _providerCallEntered, 1);
        _providerHangRelease.Wait();
    }

    private void RestoreVerifiedInputTargetForeground()
    {
        Activate();
        var handle = WindowHandle();
        if (handle != 0)
        {
            _ = SetForegroundWindow((nint)handle);
        }
        _verifiedInputTarget.BringIntoView();
        _verifiedInputTarget.Focus();
        Keyboard.Focus(_verifiedInputTarget);
    }

    [DllImport("user32.dll")]
    private static extern nint GetForegroundWindow();

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetForegroundWindow(nint hWnd);

    [DllImport("user32.dll")]
    private static extern short GetAsyncKeyState(int virtualKey);

    [DllImport("user32.dll")]
    private static extern void keybd_event(
        byte virtualKey,
        byte scanCode,
        uint flags,
        UIntPtr extraInfo);
}

internal sealed class HostileProviderElement : FrameworkElement
{
    private readonly EdgeWindow _window;

    public HostileProviderElement(EdgeWindow window)
    {
        _window = window;
    }

    protected override AutomationPeer OnCreateAutomationPeer()
    {
        return new HostileProviderPeer(this, _window);
    }
}

internal sealed class HostileProviderPeer : FrameworkElementAutomationPeer
{
    private readonly EdgeWindow _window;

    public HostileProviderPeer(HostileProviderElement owner, EdgeWindow window)
        : base(owner)
    {
        _window = window;
    }

    protected override string GetClassNameCore()
    {
        return "LocalViewW05HostileProvider";
    }

    protected override string GetNameCore()
    {
        _window.WaitIfProviderHangArmed();
        return EdgeWindow.HostileProviderName;
    }

    protected override AutomationControlType GetAutomationControlTypeCore()
    {
        return AutomationControlType.Text;
    }

    protected override bool IsControlElementCore()
    {
        return true;
    }

    protected override bool IsContentElementCore()
    {
        return true;
    }
}
