using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using DockSample.Services;

namespace DockSample.ViewModels;

public partial class DocumentViewModel : PaneViewModel
{
    private static int _counter;

    public DocumentViewModel(ILogService log)
        : base($"Document_{Guid.NewGuid():N}")
    {
        Title = $"Document {++_counter}";
        log.Write($"{Title} を作成");
    }

    [ObservableProperty] private string _text = "";

    /// <summary>×ボタンで閉じるよう要求された（MainViewModel が購読）</summary>
    public event EventHandler? CloseRequested;

    [RelayCommand]
    private void Close() => CloseRequested?.Invoke(this, EventArgs.Empty);
}
