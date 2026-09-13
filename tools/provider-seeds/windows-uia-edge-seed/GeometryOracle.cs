using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Windows.Media;

namespace LocalView.WindowsUiaEdgeSeed;

internal static class GeometryOracle
{
    private const uint MonitorDefaultToNearest = 0x00000002;
    private const uint SwpNoZOrder = 0x0004;
    private const uint SwpNoActivate = 0x0010;

    public static object ReadState(EdgeWindow window, string command)
    {
        var monitors = EnumerateMonitors();
        var capture = Capture(window, monitors);
        return StateResponse(command, capture, monitors);
    }

    public static object MoveToAlternateDpiMonitor(EdgeWindow window)
    {
        var monitors = EnumerateMonitors();
        var before = Capture(window, monitors);
        var target = monitors
            .Where(monitor =>
                monitor.Handle != before.MonitorHandle &&
                monitor.DpiX != before.MonitorDpiX)
            .OrderBy(monitor => monitor.Handle.ToInt64())
            .FirstOrDefault();

        if (target.Handle == nint.Zero)
        {
            return new
            {
                ok = false,
                command = "move_to_alternate_dpi_monitor",
                error = "mixed_dpi_precondition_unmet",
                window_handle = before.WindowHandle,
                monitor_handle = before.MonitorHandle.ToInt64(),
                window_dpi = before.WindowDpi,
                monitor_count = monitors.Count,
                distinct_dpi_count = monitors.Select(monitor => monitor.DpiX).Distinct().Count(),
                effective_dpi_values = EffectiveDpiValues(monitors),
                mixed_dpi_capable = false,
            };
        }

        var width = before.Rect.Right - before.Rect.Left;
        var height = before.Rect.Bottom - before.Rect.Top;
        var moved = SetWindowPos(
            (nint)before.WindowHandle,
            nint.Zero,
            target.Work.Left + 32,
            target.Work.Top + 32,
            width,
            height,
            SwpNoZOrder | SwpNoActivate);
        if (!moved)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "SetWindowPos failed for W10 mixed-DPI oracle");
        }

        // SetWindowPos synchronously drives the HWND DPI transition. Force the
        // WPF retained visual tree to settle before reading its independent DPI.
        window.UpdateLayout();

        var afterMonitors = EnumerateMonitors();
        var after = Capture(window, afterMonitors);
        if (after.MonitorHandle == before.MonitorHandle)
        {
            return new
            {
                ok = false,
                command = "move_to_alternate_dpi_monitor",
                error = "monitor_transition_not_observed",
                window_handle = after.WindowHandle,
                monitor_handle = after.MonitorHandle.ToInt64(),
                window_dpi = after.WindowDpi,
                monitor_count = afterMonitors.Count,
                distinct_dpi_count = afterMonitors.Select(monitor => monitor.DpiX).Distinct().Count(),
                effective_dpi_values = EffectiveDpiValues(afterMonitors),
                mixed_dpi_capable = MixedDpiCapable(afterMonitors),
            };
        }
        if (after.WindowDpi == before.WindowDpi)
        {
            return new
            {
                ok = false,
                command = "move_to_alternate_dpi_monitor",
                error = "dpi_transition_not_observed",
                window_handle = after.WindowHandle,
                monitor_handle = after.MonitorHandle.ToInt64(),
                window_dpi = after.WindowDpi,
                monitor_count = afterMonitors.Count,
                distinct_dpi_count = afterMonitors.Select(monitor => monitor.DpiX).Distinct().Count(),
                effective_dpi_values = EffectiveDpiValues(afterMonitors),
                mixed_dpi_capable = MixedDpiCapable(afterMonitors),
            };
        }

        return StateResponse("move_to_alternate_dpi_monitor", after, afterMonitors);
    }

    private static object StateResponse(
        string command,
        GeometryCapture capture,
        IReadOnlyList<MonitorSnapshot> monitors)
    {
        return new
        {
            ok = true,
            command,
            window_handle = capture.WindowHandle,
            monitor_handle = capture.MonitorHandle.ToInt64(),
            monitor_dpi = capture.MonitorDpiX,
            window_dpi = capture.WindowDpi,
            window_left = capture.Rect.Left,
            window_top = capture.Rect.Top,
            window_right = capture.Rect.Right,
            window_bottom = capture.Rect.Bottom,
            monitor_count = monitors.Count,
            distinct_dpi_count = monitors.Select(monitor => monitor.DpiX).Distinct().Count(),
            effective_dpi_values = EffectiveDpiValues(monitors),
            mixed_dpi_capable = MixedDpiCapable(monitors),
        };
    }

    private static uint[] EffectiveDpiValues(IReadOnlyList<MonitorSnapshot> monitors)
    {
        return monitors
            .Select(monitor => monitor.DpiX)
            .Distinct()
            .OrderBy(dpi => dpi)
            .ToArray();
    }

    private static GeometryCapture Capture(
        EdgeWindow window,
        IReadOnlyList<MonitorSnapshot> monitors)
    {
        var windowHandle = window.WindowHandle();
        if (windowHandle == 0)
        {
            throw new InvalidOperationException("W10 geometry oracle requires a live HWND");
        }
        if (!GetWindowRect((nint)windowHandle, out var rect))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "GetWindowRect failed for W10 geometry oracle");
        }

        var monitorHandle = MonitorFromWindow((nint)windowHandle, MonitorDefaultToNearest);
        if (monitorHandle == nint.Zero)
        {
            throw new InvalidOperationException("W10 geometry oracle could not resolve the target monitor");
        }
        var monitor = monitors.FirstOrDefault(candidate => candidate.Handle == monitorHandle);
        if (monitor.Handle == nint.Zero)
        {
            throw new InvalidOperationException("W10 geometry oracle monitor enumeration lost the target monitor");
        }

        // Independent from UI Automation and from production GetDpiForWindow:
        // WPF reports the live DPI of the retained visual tree.
        var visualDpi = VisualTreeHelper.GetDpi(window);
        var windowDpi = checked((uint)Math.Round(
            visualDpi.PixelsPerInchX,
            MidpointRounding.AwayFromZero));

        return new GeometryCapture(
            windowHandle,
            monitorHandle,
            monitor.DpiX,
            windowDpi,
            rect);
    }

    private static bool MixedDpiCapable(IReadOnlyList<MonitorSnapshot> monitors)
    {
        return monitors.Count >= 2 &&
            monitors.Select(monitor => monitor.DpiX).Distinct().Count() >= 2;
    }

    private static List<MonitorSnapshot> EnumerateMonitors()
    {
        var monitors = new List<MonitorSnapshot>();
        MonitorEnumProc callback = (
            nint monitorHandle,
            nint monitorDeviceContext,
            ref NativeRect monitorRect,
            nint data) =>
        {
            var info = new MonitorInfo
            {
                Size = (uint)Marshal.SizeOf<MonitorInfo>(),
            };
            if (!GetMonitorInfo(monitorHandle, ref info))
            {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "GetMonitorInfo failed for W10 geometry oracle");
            }

            var result = GetDpiForMonitor(
                monitorHandle,
                MonitorDpiType.Effective,
                out var dpiX,
                out var dpiY);
            if (result != 0)
            {
                Marshal.ThrowExceptionForHR(result);
            }

            monitors.Add(new MonitorSnapshot(
                monitorHandle,
                info.Monitor,
                info.Work,
                dpiX,
                dpiY));
            return true;
        };

        if (!EnumDisplayMonitors(nint.Zero, nint.Zero, callback, nint.Zero))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "EnumDisplayMonitors failed for W10 geometry oracle");
        }
        if (monitors.Count == 0)
        {
            throw new InvalidOperationException("W10 geometry oracle requires at least one attached display");
        }
        return monitors;
    }

    private enum MonitorDpiType
    {
        Effective = 0,
    }

    private readonly record struct GeometryCapture(
        long WindowHandle,
        nint MonitorHandle,
        uint MonitorDpiX,
        uint WindowDpi,
        NativeRect Rect);

    private readonly record struct MonitorSnapshot(
        nint Handle,
        NativeRect Monitor,
        NativeRect Work,
        uint DpiX,
        uint DpiY);

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeRect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MonitorInfo
    {
        public uint Size;
        public NativeRect Monitor;
        public NativeRect Work;
        public uint Flags;
    }

    private delegate bool MonitorEnumProc(
        nint monitorHandle,
        nint monitorDeviceContext,
        ref NativeRect monitorRect,
        nint data);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool EnumDisplayMonitors(
        nint deviceContext,
        nint clipRect,
        MonitorEnumProc callback,
        nint data);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetMonitorInfo(nint monitorHandle, ref MonitorInfo monitorInfo);

    [DllImport("user32.dll")]
    private static extern nint MonitorFromWindow(nint windowHandle, uint flags);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetWindowRect(nint windowHandle, out NativeRect rect);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowPos(
        nint windowHandle,
        nint insertAfter,
        int x,
        int y,
        int width,
        int height,
        uint flags);

    [DllImport("Shcore.dll")]
    private static extern int GetDpiForMonitor(
        nint monitorHandle,
        MonitorDpiType dpiType,
        out uint dpiX,
        out uint dpiY);
}
