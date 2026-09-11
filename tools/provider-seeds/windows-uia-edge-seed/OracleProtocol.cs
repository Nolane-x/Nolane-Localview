using System.Diagnostics;
using System.Text.Json;
using System.Windows.Interop;

namespace LocalView.WindowsUiaEdgeSeed;

internal sealed class OracleProtocol
{
    private readonly EdgeWindow _window;
    private readonly string _seedRunId;

    public OracleProtocol(EdgeWindow window, string seedRunId)
    {
        _window = window;
        _seedRunId = seedRunId;
    }

    public void Start()
    {
        var thread = new Thread(Run)
        {
            IsBackground = true,
            Name = "localview-windows-uia-edge-oracle",
        };
        thread.Start();
    }

    private void Run()
    {
        string? line;
        while ((line = Console.In.ReadLine()) is not null)
        {
            HandleLine(line);
        }
    }

    private void HandleLine(string line)
    {
        try
        {
            using var document = JsonDocument.Parse(line);
            if (!document.RootElement.TryGetProperty("command", out var commandElement))
            {
                Write(new { ok = false, error = "missing_command" });
                return;
            }

            var command = commandElement.GetString();
            switch (command)
            {
                case "get_ground_truth":
                    Write(ReadGroundTruth());
                    break;
                case "get_virtual_item_state":
                    Write(ReadVirtualItemState());
                    break;
                case "shutdown":
                    Write(new { ok = true, command = "shutdown" });
                    _window.Dispatcher.BeginInvoke(() => _window.Close());
                    break;
                default:
                    Write(new { ok = false, error = "unknown_command", command });
                    break;
            }
        }
        catch (Exception error)
        {
            Write(new
            {
                ok = false,
                error = "oracle_exception",
                detail = error.GetType().Name,
            });
        }
    }

    private object ReadGroundTruth()
    {
        return _window.Dispatcher.Invoke(() => new
        {
            ok = true,
            command = "get_ground_truth",
            seed_run_id = _seedRunId,
            process_id = Environment.ProcessId,
            window_handle = WindowHandle(),
            virtual_item_index = EdgeWindow.VirtualItemIndex,
            virtual_item_name = EdgeWindow.VirtualItemName,
            virtual_item_container_generated = _window.IsVirtualItemContainerGenerated(),
        });
    }

    private object ReadVirtualItemState()
    {
        return _window.Dispatcher.Invoke(() => new
        {
            ok = true,
            command = "get_virtual_item_state",
            seed_run_id = _seedRunId,
            virtual_item_index = EdgeWindow.VirtualItemIndex,
            virtual_item_name = EdgeWindow.VirtualItemName,
            virtual_item_container_generated = _window.IsVirtualItemContainerGenerated(),
        });
    }

    private long WindowHandle()
    {
        return new WindowInteropHelper(_window).Handle.ToInt64();
    }

    private static void Write(object response)
    {
        Console.Out.WriteLine(JsonSerializer.Serialize(response));
        Console.Out.Flush();
    }
}
