using CommunityToolkit.Mvvm.ComponentModel;

namespace DockSample.ViewModels;

public abstract partial class PaneViewModel : ObservableObject
{
    protected PaneViewModel(string contentId) => ContentId = contentId;

    /// <summary>レイアウト保存/復元時に ViewModel を特定するためのID</summary>
    public string ContentId { get; }

    [ObservableProperty] private string _title = "";
    [ObservableProperty] private bool _isSelected;
    [ObservableProperty] private bool _isActive;
}
