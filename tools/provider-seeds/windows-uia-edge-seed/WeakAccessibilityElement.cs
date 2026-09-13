using System.Globalization;
using System.Threading;
using System.Windows;
using System.Windows.Automation.Peers;
using System.Windows.Input;
using System.Windows.Media;

namespace LocalView.WindowsUiaEdgeSeed;

internal sealed class WeakAccessibilityElement : FrameworkElement
{
    public const string VisualOnlyLabel = "W14_VISUAL_ONLY_ACTION";

    private int _visualEffectCount;

    public WeakAccessibilityElement()
    {
        Visibility = Visibility.Collapsed;
    }

    public int VisualEffectCount => Volatile.Read(ref _visualEffectCount);

    public Rect VisualHotspotBounds
    {
        get
        {
            var width = System.Math.Max(0, ActualWidth - 16);
            var height = System.Math.Max(0, ActualHeight - 16);
            return new Rect(8, 8, width, height);
        }
    }

    public void Reset()
    {
        Interlocked.Exchange(ref _visualEffectCount, 0);
        Visibility = Visibility.Visible;
        InvalidateVisual();
    }

    protected override AutomationPeer OnCreateAutomationPeer()
    {
        return new WeakAccessibilityPeer(this);
    }

    protected override void OnRender(DrawingContext drawingContext)
    {
        base.OnRender(drawingContext);
        var bounds = VisualHotspotBounds;
        drawingContext.DrawRectangle(Brushes.DimGray, new Pen(Brushes.Black, 1), bounds);

        var text = new FormattedText(
            VisualOnlyLabel,
            CultureInfo.InvariantCulture,
            FlowDirection.LeftToRight,
            new Typeface("Segoe UI"),
            14,
            Brushes.White,
            VisualTreeHelper.GetDpi(this).PixelsPerDip);
        var origin = new Point(
            bounds.X + System.Math.Max(0, (bounds.Width - text.Width) / 2),
            bounds.Y + System.Math.Max(0, (bounds.Height - text.Height) / 2));
        drawingContext.DrawText(text, origin);
    }

    protected override void OnMouseLeftButtonDown(MouseButtonEventArgs e)
    {
        base.OnMouseLeftButtonDown(e);
        if (VisualHotspotBounds.Contains(e.GetPosition(this)))
        {
            Interlocked.Increment(ref _visualEffectCount);
            e.Handled = true;
        }
    }
}

internal sealed class WeakAccessibilityPeer : FrameworkElementAutomationPeer
{
    public WeakAccessibilityPeer(WeakAccessibilityElement owner)
        : base(owner)
    {
    }

    protected override string GetClassNameCore()
    {
        return "LocalViewW14OwnerDrawn";
    }

    protected override string GetNameCore()
    {
        return "LocalView W14 owner-drawn surface";
    }

    protected override AutomationControlType GetAutomationControlTypeCore()
    {
        return AutomationControlType.Custom;
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
