using System;
using System.Windows;

namespace LocalView.WindowsUiaEdgeSeed;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        var application = new Application();
        var window = new EdgeWindow();
        var oracle = new OracleProtocol(window, Guid.NewGuid().ToString("N"));
        window.SourceInitialized += (_, _) => oracle.Start();
        application.Run(window);
    }
}
