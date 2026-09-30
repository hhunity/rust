using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Messaging;
using CommunityToolkit.Mvvm.Messaging.Messages;
using DockSample.Services;

namespace DockSample.ViewModels;

public enum ToolLocation { Left, Right, Bottom }

public abstract class ToolViewModel : PaneViewModel
{
    protected ToolViewModel(string contentId, string title, ToolLocation location)
        : base(contentId)
    {
        Title = title;
        PreferredLocation = location;
    }

    /// <summary>初めて開くときにドッキングする場所</summary>
    public ToolLocation PreferredLocation { get; }
}

// ---- メッセージ ----
public sealed class SelectedItemChangedMessage(string? value) : ValueChangedMessage<string?>(value);

// ---- エクスプローラー（左） ----
public partial class ExplorerViewModel : ToolViewModel
{
    private readonly IMessenger _messenger;
    private readonly ILogService _log;

    public ExplorerViewModel(IMessenger messenger, ILogService log)
        : base("Tool_Explorer", "エクスプローラー", ToolLocation.Left)
    {
        _messenger = messenger;
        _log = log;
    }

    public ObservableCollection<string> Items { get; } =
        new() { "Apple", "Banana", "Cherry", "Durian", "Elderberry" };

    [ObservableProperty] private string? _selectedItem;

    partial void OnSelectedItemChanged(string? value)
    {
        _log.Write($"Explorer: '{value}' を選択");
        _messenger.Send(new SelectedItemChangedMessage(value));
    }
}

// ---- プロパティ（右） ----
public partial class PropertiesViewModel : ToolViewModel, IRecipient<SelectedItemChangedMessage>
{
    public PropertiesViewModel(IMessenger messenger)
        : base("Tool_Properties", "プロパティ", ToolLocation.Right)
    {
        messenger.RegisterAll(this);
    }

    [ObservableProperty] private string? _selectedName;
    [ObservableProperty] private int _length;

    public void Receive(SelectedItemChangedMessage message)
    {
        SelectedName = message.Value;
        Length = message.Value?.Length ?? 0;
    }
}

// ---- 出力（下） ----
public class OutputViewModel : ToolViewModel
{
    public OutputViewModel(ILogService log)
        : base("Tool_Output", "出力", ToolLocation.Bottom)
    {
        Log = log;
    }

    public ILogService Log { get; }
}
