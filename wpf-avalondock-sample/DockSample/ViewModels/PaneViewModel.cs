using CommunityToolkit.Mvvm.ComponentModel;

namespace DockSample.ViewModels;

/// <summary>初めて表示するときにドッキングする場所</summary>
public enum DockLocation { Left, Right, Bottom, Document }

/// <summary>
/// ドッキングペインの共通基底クラス。
/// ドキュメントもツールも同じ仕組み（AvalonDock の Anchorable）で表示し、
/// IsVisible で表示/非表示を切り替える。
/// </summary>
public abstract partial class PaneViewModel : ObservableObject
{
    protected PaneViewModel(string contentId, string title, DockLocation location)
    {
        ContentId = contentId;
        Title = title;
        PreferredLocation = location;
    }

    /// <summary>レイアウト保存/復元時に ViewModel を特定するためのID（固定値にすること）</summary>
    public string ContentId { get; }

    public DockLocation PreferredLocation { get; }

    [ObservableProperty] private string _title;
    [ObservableProperty] private bool _isSelected;
    [ObservableProperty] private bool _isActive;

    /// <summary>表示中かどうか（ToggleButton と ×ボタンの両方から変わる）</summary>
    [ObservableProperty] private bool _isVisible;

    partial void OnIsVisibleChanged(bool value)
    {
        if (value) IsSelected = true;   // 表示したらそのタブを前面に
    }
}
