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
    /// <param name="title">タブ・タイトルバー・フローティングウィンドウに表示する名前</param>
    /// <param name="buttonText">上部のトグルボタンに表示する名前（省略時は title と同じ）</param>
    protected PaneViewModel(string contentId, string title, DockLocation location, string? buttonText = null)
    {
        ContentId = contentId;
        Title = title;
        ButtonText = buttonText ?? title;
        PreferredLocation = location;
    }

    /// <summary>レイアウト保存/復元時に ViewModel を特定するためのID（固定値にすること）</summary>
    public string ContentId { get; }

    public DockLocation PreferredLocation { get; }

    /// <summary>タブ・タイトルバー・フローティングウィンドウのタイトル</summary>
    [ObservableProperty] private string _title;

    /// <summary>上部のトグルボタンの表示</summary>
    [ObservableProperty] private string _buttonText;
    [ObservableProperty] private bool _isSelected;
    [ObservableProperty] private bool _isActive;

    /// <summary>表示中かどうか（ToggleButton と ×ボタンの両方から変わる）</summary>
    [ObservableProperty] private bool _isVisible;

    partial void OnIsVisibleChanged(bool value)
    {
        if (value) IsSelected = true;   // 表示したらそのタブを前面に
    }
}
