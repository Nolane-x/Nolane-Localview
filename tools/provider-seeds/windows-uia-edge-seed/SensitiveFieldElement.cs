using System.Threading;
using System.Windows;
using System.Windows.Automation.Peers;
using System.Windows.Automation.Provider;

namespace LocalView.WindowsUiaEdgeSeed;

internal sealed class SensitiveFieldElement : FrameworkElement
{
    private string _canary = string.Empty;
    private int _valueReadCount;

    public void Prepare(string canary)
    {
        if (string.IsNullOrWhiteSpace(canary))
        {
            throw new ArgumentException("sensitive canary must be non-empty", nameof(canary));
        }

        _canary = canary;
        Interlocked.Exchange(ref _valueReadCount, 0);
    }

    public int SecretLength => _canary.Length;

    public int ValueReadCount => Volatile.Read(ref _valueReadCount);

    internal string ReadValueForProvider()
    {
        Interlocked.Increment(ref _valueReadCount);
        return _canary;
    }

    protected override AutomationPeer OnCreateAutomationPeer()
    {
        return new SensitiveFieldPeer(this);
    }
}

internal sealed class SensitiveFieldPeer : FrameworkElementAutomationPeer, IValueProvider
{
    private readonly SensitiveFieldElement _owner;

    public SensitiveFieldPeer(SensitiveFieldElement owner)
        : base(owner)
    {
        _owner = owner;
    }

    public override object? GetPattern(PatternInterface patternInterface)
    {
        return patternInterface == PatternInterface.Value ? this : base.GetPattern(patternInterface);
    }

    bool IValueProvider.IsReadOnly => true;

    string IValueProvider.Value => _owner.ReadValueForProvider();

    void IValueProvider.SetValue(string value)
    {
        throw new InvalidOperationException("W13 sensitive seed is read-only");
    }

    protected override string GetClassNameCore()
    {
        return "LocalViewW13SensitiveField";
    }

    protected override string GetNameCore()
    {
        return "LocalView W13 protected field";
    }

    protected override AutomationControlType GetAutomationControlTypeCore()
    {
        return AutomationControlType.Edit;
    }

    protected override bool IsPasswordCore()
    {
        return true;
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
