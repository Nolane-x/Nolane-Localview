using System.Collections.ObjectModel;
using System.Threading;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Automation.Peers;
using System.Windows.Controls;

namespace LocalView.WindowsUiaEdgeSeed;

internal sealed class EdgeWindow : Window
{
    public const int VirtualItemIndex = 255;
    public const string VirtualItemName = "LocalView Virtual Item 255";
    public const string VirtualizedListAutomationId = "LocalViewW03VirtualizedItems";
    public const string VirtualizedListName = "LocalView W03 Virtualized Items";
    public const string HostileProviderAutomationId = "LocalViewW05HostileProvider";
    public const string HostileProviderName = "LocalView W05 Hostile Provider";

    private readonly ListBox _virtualizedList;
    private readonly ManualResetEventSlim _providerHangRelease = new(false);
    private int _providerHangArmed;
    private int _providerCallEntered;

    public EdgeWindow()
    {
        Title = "LocalView Windows UIA Edge Seed";
        Width = 520;
        Height = 240;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;

        var items = new ObservableCollection<string>(
            Enumerable.Range(0, VirtualItemIndex + 1)
                .Select(index => $"LocalView Virtual Item {index}"));

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
        content.Children.Add(_virtualizedList);
        Content = content;

        Loaded += (_, _) =>
        {
            UpdateLayout();
            _virtualizedList.UpdateLayout();
        };
    }

    public bool IsVirtualItemContainerGenerated()
    {
        return _virtualizedList.ItemContainerGenerator.ContainerFromIndex(VirtualItemIndex) is not null;
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
