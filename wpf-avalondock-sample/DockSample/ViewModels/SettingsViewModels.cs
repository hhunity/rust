using System.Collections.ObjectModel;
using System.ComponentModel;
using System.ComponentModel.DataAnnotations;
using CommunityToolkit.Mvvm.ComponentModel;
using DockSample.Services;

namespace DockSample.ViewModels;

/// <summary>
/// 設定ペインの左タブ1ページ分の基底。
/// ObservableValidator なので INotifyDataErrorInfo（HasErrors / GetErrors）付き。
/// </summary>
public abstract class SettingsPageViewModel : ObservableValidator
{
    protected SettingsPageViewModel(string header)
    {
        Header = header;
        // エラーが増減したら、ツールチップ用の文字列も更新する
        ErrorsChanged += (_, _) => OnPropertyChanged(nameof(ErrorSummary));
    }

    /// <summary>タブに表示する名前（\n で改行できる）</summary>
    public string Header { get; }

    /// <summary>タブのツールチップに出すエラー内容</summary>
    public string? ErrorSummary => HasErrors
        ? string.Join("\n", GetErrors().Select(e => e.ErrorMessage))
        : null;
}

// ---- 全般 ----
public partial class GeneralPageViewModel : SettingsPageViewModel
{
    public GeneralPageViewModel() : base("全般")
    {
        ValidateAllProperties();   // 起動直後から未入力をエラーとして表示する
    }

    [ObservableProperty]
    [NotifyDataErrorInfo]
    [Required(ErrorMessage = "ユーザー名は必須です")]
    [MaxLength(20, ErrorMessage = "ユーザー名は20文字以内です")]
    private string _userName = "";

    /// <summary>チェックボックスと、下に出てくる詳細設定の両方がこれを見る</summary>
    [ObservableProperty] private bool _autoSave = true;

    // ---- 自動保存が ON のときだけ表示される設定 ----
    [ObservableProperty]
    [NotifyDataErrorInfo]
    [Range(1, 60, ErrorMessage = "保存間隔は 1～60 分で指定してください")]
    private int _autoSaveInterval = 5;

    [ObservableProperty] private bool _saveOnClose = true;

    // 隠れている項目のエラーでタブに印が出ないよう、OFF ならエラーを消し、ON で検証し直す
    partial void OnAutoSaveChanged(bool value)
    {
        if (value)
            ValidateProperty(AutoSaveInterval, nameof(AutoSaveInterval));
        else
            ClearErrors(nameof(AutoSaveInterval));
    }
}

// ---- 外観 ----
public partial class AppearancePageViewModel : SettingsPageViewModel
{
    private readonly IAppearanceService _appearance;

    public AppearancePageViewModel(IAppearanceService appearance) : base("外観と\nフォント設定")
    {
        _appearance = appearance;
    }

    [ObservableProperty]
    [NotifyDataErrorInfo]
    [Range(8, 32, ErrorMessage = "フォントサイズは 8～32 で指定してください")]
    private double _fontSize = 12;

    // 範囲内の値になったら、アプリ全体の文字サイズに反映する
    partial void OnFontSizeChanged(double value)
    {
        if (value is >= 8 and <= 32)
            _appearance.ApplyBaseFontSize(value);
    }
}

// ---- 設定ペイン本体（左にタブ） ----
public partial class SettingsViewModel : PaneViewModel
{
    /// <param name="pages">DI に SettingsPageViewModel として登録したページ（登録順にタブが並ぶ）</param>
    public SettingsViewModel(IEnumerable<SettingsPageViewModel> pages)
        : base("Tool_Settings", "設定", DockLocation.Right)
    {
        Pages = new ObservableCollection<SettingsPageViewModel>(pages);
        _selectedPage = Pages.FirstOrDefault();

        // どれかのページのエラー状態が変わったら、ペイン全体の HasErrors も通知する
        foreach (var page in Pages)
            page.PropertyChanged += OnPagePropertyChanged;
    }

    public ObservableCollection<SettingsPageViewModel> Pages { get; }

    [ObservableProperty] private SettingsPageViewModel? _selectedPage;

    /// <summary>どれかのページにエラーがあれば true（上部のボタンに印を出す）</summary>
    public override bool HasErrors => Pages.Any(p => p.HasErrors);

    private void OnPagePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(SettingsPageViewModel.HasErrors))
            OnPropertyChanged(nameof(HasErrors));
    }
}
