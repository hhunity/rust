using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace DockSample.ViewModels;

public abstract partial class PaneViewModel : ObservableObject
{
    protected PaneViewModel(string contentId) => ContentId = contentId;

    public string ContentId { get; }

    [ObservableProperty] private string _title = "";
    [ObservableProperty] private bool _isSelected;
    [ObservableProperty] private bool _isActive;

    /// <summary>×ボタンで閉じるよう要求された（MainViewModel が購読）</summary>
    public event EventHandler? CloseRequested;

    [RelayCommand]
    private void Close() => CloseRequested?.Invoke(this, EventArgs.Empty);
}
