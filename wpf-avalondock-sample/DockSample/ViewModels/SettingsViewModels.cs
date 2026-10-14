using System.Collections.ObjectModel;
using System.ComponentModel;
using System.ComponentModel.DataAnnotations;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using DockSample.Services;
using DockSample.Settings;

namespace DockSample.ViewModels;

/// <summary>
/// 設定ペインの左タブ1ページ分の基底。
/// ObservableValidator なので INotifyDataErrorInfo（HasErrors / GetErrors）付き。
///
/// 設定（AppSettings）との対応付けは各ページで手書きする：
///   ・設定 → 画面 : LoadFrom（起動時と「元に戻す」のときに呼ばれる）
///   ・画面 → 設定 : 各プロパティの OnXxxChanged で Write(...) を呼ぶ
/// 項目を追加したら [ObservableProperty]・OnXxxChanged・LoadFrom の3か所を書くこと。
/// </summary>
public abstract class SettingsPageViewModel : ObservableValidator
{
    private bool _loading;

    protected SettingsPageViewModel(string header, ISettingsService settings)
    {
        Header = header;
        SettingsService = settings;

        // エラーが増減したら、ツールチップ用の文字列も更新する
        ErrorsChanged += (_, _) => OnPropertyChanged(nameof(ErrorSummary));

        // 「元に戻す」されたら、設定から読み直す
        settings.Reverted += (_, _) => Load();
    }

    protected ISettingsService SettingsService { get; }

    /// <summary>タブに表示する名前（\n で改行できる）</summary>
    public string Header { get; }

    /// <summary>タブのツールチップに出すエラー内容</summary>
    public string? ErrorSummary => HasErrors
        ? string.Join("\n", GetErrors().Select(e => e.ErrorMessage))
        : null;

    /// <summary>設定 → 画面。各ページで、設定の値を ViewModel のプロパティに入れる</summary>
    protected abstract void LoadFrom(AppSettings s);

    /// <summary>
    /// 設定を読み込む。派生クラスのコンストラクタの最後で呼ぶこと
    /// （基底のコンストラクタから呼ぶと、派生クラスのフィールドがまだ準備できていない）
    /// </summary>
    protected void Load()
    {
        _loading = true;   // 読み込み中は Write で書き戻さない
        try { LoadFrom(SettingsService.Current); }
        finally { _loading = false; }
    }

    /// <summary>画面 → 設定。OnXxxChanged から呼び、設定に書き写して「未保存の変更」を更新する</summary>
    protected void Write(Action<AppSettings> write)
    {
        if (_loading) return;
        write(SettingsService.Current);
        SettingsService.NotifyChanged();
    }
}

// ---- 全般 ----
public partial class GeneralPageViewModel : SettingsPageViewModel
{
    public GeneralPageViewModel(ISettingsService settings) : base("全般", settings)
    {
        Load();
        ValidateAllProperties();   // 起動直後から未入力をエラーとして表示する
        if (!AutoSave) ClearErrors(nameof(AutoSaveInterval));   // 隠れている項目のエラーは出さない
    }

    // 設定 → 画面（プロパティと同じ順に並べておくと、書き忘れに気付きやすい）
    protected override void LoadFrom(AppSettings s)
    {
        UserName         = s.General.UserName;
        AutoSave         = s.General.AutoSave.Enabled;
        AutoSaveInterval = s.General.AutoSave.Interval;
        SaveOnClose      = s.General.AutoSave.OnClose;
    }

    // ---- ユーザー名 ----
    [ObservableProperty]
    [NotifyDataErrorInfo]
    [Required(ErrorMessage = "ユーザー名は必須です")]
    [MaxLength(20, ErrorMessage = "ユーザー名は20文字以内です")]
    private string _userName = "";

    partial void OnUserNameChanged(string value) =>
        Write(s => s.General.UserName = value);

    // ---- 自動保存（チェックボックスと、下に出てくる詳細設定の両方がこれを見る） ----
    [ObservableProperty] private bool _autoSave = true;

    partial void OnAutoSaveChanged(bool value)
    {
        Write(s => s.General.AutoSave.Enabled = value);

        // 隠れている項目のエラーでタブに印が出ないよう、OFF ならエラーを消し、ON で検証し直す
        if (value)
            ValidateProperty(AutoSaveInterval, nameof(AutoSaveInterval));
        else
            ClearErrors(nameof(AutoSaveInterval));
    }

    // ---- 自動保存が ON のときだけ表示される設定 ----
    [ObservableProperty]
    [NotifyDataErrorInfo]
    [Range(1, 60, ErrorMessage = "保存間隔は 1～60 分で指定してください")]
    private int _autoSaveInterval = 5;

    partial void OnAutoSaveIntervalChanged(int value) =>
        Write(s => s.General.AutoSave.Interval = value);

    [ObservableProperty] private bool _saveOnClose = true;

    partial void OnSaveOnCloseChanged(bool value) =>
        Write(s => s.General.AutoSave.OnClose = value);
}

// ---- 外観 ----
public partial class AppearancePageViewModel : SettingsPageViewModel
{
    private readonly IAppearanceService _appearance;

    public AppearancePageViewModel(IAppearanceService appearance, ISettingsService settings)
        : base("外観と\nフォント設定", settings)
    {
        _appearance = appearance;
        Load();   // FontSize が変われば OnFontSizeChanged が呼ばれ、起動時に文字サイズも反映される
    }

    protected override void LoadFrom(AppSettings s)
    {
        FontSize = s.Appearance.FontSize;
    }

    // ---- 文字サイズ ----
    [ObservableProperty]
    [NotifyDataErrorInfo]
    [Range(8, 32, ErrorMessage = "フォントサイズは 8～32 で指定してください")]
    private double _fontSize = 12;

    partial void OnFontSizeChanged(double value)
    {
        Write(s => s.Appearance.FontSize = value);

        // 範囲内の値になったら、アプリ全体の文字サイズに反映する
        if (value is >= 8 and <= 32)
            _appearance.ApplyBaseFontSize(value);
    }
}

// ---- 設定ペイン本体（左にタブ） ----
public partial class SettingsViewModel : PaneViewModel
{
    private readonly ISettingsService _settings;

    /// <param name="pages">DI に SettingsPageViewModel として登録したページ（登録順にタブが並ぶ）</param>
    public SettingsViewModel(IEnumerable<SettingsPageViewModel> pages, ISettingsService settings)
        : base("Tool_Settings", "設定", DockLocation.Right)
    {
        _settings = settings;
        Pages = new ObservableCollection<SettingsPageViewModel>(pages);
        _selectedPage = Pages.FirstOrDefault();

        // どれかのページのエラー状態が変わったら、ペイン全体の HasErrors も通知する
        foreach (var page in Pages)
            page.PropertyChanged += OnPagePropertyChanged;

        // 未保存の変更の有無が変わったら、表示とボタンの押せる/押せないを更新する
        _settings.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName != nameof(ISettingsService.IsDirty)) return;
            OnPropertyChanged(nameof(IsDirty));
            SaveCommand.NotifyCanExecuteChanged();
            RevertCommand.NotifyCanExecuteChanged();
        };
    }

    /// <summary>最後に保存してから変更があるか（「未保存」の表示に使う）</summary>
    public bool IsDirty => _settings.IsDirty;

    /// <summary>保存：変更があり、エラーがないときだけ押せる</summary>
    [RelayCommand(CanExecute = nameof(CanSave))]
    private void Save() => _settings.Save();
    private bool CanSave() => _settings.IsDirty && !HasErrors;

    /// <summary>元に戻す：最後に保存した状態に戻す。変更があるときだけ押せる</summary>
    [RelayCommand(CanExecute = nameof(CanRevert))]
    private void Revert() => _settings.Revert();
    private bool CanRevert() => _settings.IsDirty;

    public ObservableCollection<SettingsPageViewModel> Pages { get; }

    [ObservableProperty] private SettingsPageViewModel? _selectedPage;

    /// <summary>どれかのページにエラーがあれば true（上部のボタンに印を出す）</summary>
    public override bool HasErrors => Pages.Any(p => p.HasErrors);

    private void OnPagePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(SettingsPageViewModel.HasErrors))
        {
            OnPropertyChanged(nameof(HasErrors));
            SaveCommand.NotifyCanExecuteChanged();   // エラーがある間は保存できない
        }
    }
}
