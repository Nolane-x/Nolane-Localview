using System.Collections.ObjectModel;
using System.Windows;
using System.Windows.Controls;

namespace LocalView.WindowsUiaEdgeSeed;

internal sealed class EdgeWindow : Window
{
    public const int VirtualItemIndex = 255;
    public const string VirtualItemName = "LocalView Virtual Item 255";

    private readonly ListBox _virtualizedList;

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

        VirtualizingStackPanel.SetIsVirtualizing(_virtualizedList, true);
        VirtualizingStackPanel.SetVirtualizationMode(
            _virtualizedList,
            VirtualizationMode.Recycling);
        ScrollViewer.SetCanContentScroll(_virtualizedList, true);

        Content = _virtualizedList;

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
}
