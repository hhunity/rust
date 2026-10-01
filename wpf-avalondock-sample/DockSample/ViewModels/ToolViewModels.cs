using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Messaging;
using CommunityToolkit.Mvvm.Messaging.Messages;
using DockSample.Services;

namespace DockSample.ViewModels;

// ---- メッセージ ----
public sealed class SelectedItemChangedMessage(string? value) : ValueChangedMessage<string?>(value);

// ---- エクスプローラー（左） ----
public partial class ExplorerViewModel : PaneViewModel
{
    private readonly IMessenger _messenger;
    private readonly ILogService _log;

    public ExplorerViewModel(IMessenger messenger, ILogService log)
        : base("Tool_Explorer", "エクスプローラー", DockLocation.Left)
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
public partial class PropertiesViewModel : PaneViewModel, IRecipient<SelectedItemChangedMessage>
{
    public PropertiesViewModel(IMessenger messenger)
        : base("Tool_Properties", "プロパティ", DockLocation.Right)
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
public class OutputViewModel : PaneViewModel
{
    public OutputViewModel(ILogService log)
        : base("Tool_Output", "出力", DockLocation.Bottom)
    {
        Log = log;
    }

    public ILogService Log { get; }
}
