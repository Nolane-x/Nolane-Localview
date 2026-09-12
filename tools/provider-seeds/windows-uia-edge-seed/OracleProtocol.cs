using System.Text.Json;

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
                case "prepare_verified_input_target":
                    Write(_window.Dispatcher.Invoke(() =>
                    {
                        _window.PrepareVerifiedInputTarget();
                        return ReadVerifiedInputStateOnUiThread("prepare_verified_input_target");
                    }));
                    break;
                case "get_verified_input_state":
                    Write(ReadVerifiedInputState("get_verified_input_state"));
                    break;
                case "steal_foreground":
                    Write(_window.Dispatcher.Invoke(() =>
                    {
                        _window.StealForeground();
                        return ReadVerifiedInputStateOnUiThread("steal_foreground");
                    }));
                    break;
                case "hold_shift":
                    Write(_window.Dispatcher.Invoke(() =>
                    {
                        _window.HoldShiftFixture();
                        return ReadVerifiedInputStateOnUiThread("hold_shift");
                    }));
                    break;
                case "release_shift":
                    Write(_window.Dispatcher.Invoke(() =>
                    {
                        _window.ReleaseShiftFixture();
                        return ReadVerifiedInputStateOnUiThread("release_shift");
                    }));
                    break;
                case "arm_provider_hang":
                    _window.ArmProviderHang();
                    Write(new
                    {
                        ok = true,
                        command = "arm_provider_hang",
                        hang_armed = _window.IsProviderHangArmed(),
                    });
                    break;
                case "get_provider_hang_state":
                    Write(new
                    {
                        ok = true,
                        command = "get_provider_hang_state",
                        hang_armed = _window.IsProviderHangArmed(),
                        provider_call_entered = _window.ProviderCallEntered(),
                    });
                    break;
                case "release_provider_hang":
                    _window.ReleaseProviderHang();
                    Write(new
                    {
                        ok = true,
                        command = "release_provider_hang",
                        hang_armed = _window.IsProviderHangArmed(),
                    });
                    break;
                case "shutdown":
                    _window.ReleaseProviderHang();
                    _window.Dispatcher.Invoke(_window.CleanupVerifiedInputFixture);
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
                message = error.Message,
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
            window_handle = _window.WindowHandle(),
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

    private object ReadVerifiedInputState(string command)
    {
        return _window.Dispatcher.Invoke(() => ReadVerifiedInputStateOnUiThread(command));
    }

    private object ReadVerifiedInputStateOnUiThread(string command)
    {
        return new
        {
            ok = true,
            command,
            seed_run_id = _seedRunId,
            process_id = Environment.ProcessId,
            target_automation_id = EdgeWindow.VerifiedInputTargetAutomationId,
            window_handle = _window.WindowHandle(),
            thief_window_handle = _window.ForegroundThiefWindowHandle(),
            foreground_window_handle = _window.ForegroundWindowHandle(),
            target_is_foreground = _window.IsTargetForeground(),
            thief_is_foreground = _window.IsThiefForeground(),
            shift_down = _window.IsShiftDown(),
            effect_count = _window.VerifiedInputEffectCount(),
        };
    }

    private static void Write(object response)
    {
        Console.Out.WriteLine(JsonSerializer.Serialize(response));
        Console.Out.Flush();
    }
}
