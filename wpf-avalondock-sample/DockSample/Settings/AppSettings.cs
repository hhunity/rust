namespace DockSample.Settings;

/// <summary>
/// 保存する設定（ただのデータ）。設定フォルダの中に、セクションごとのファイルとして保存される
/// （General → general.json、Appearance → appearance.json。対応は JsonSettingsService.Sections）。
/// ・ここに書いた初期値が、設定ファイルがまだないとき / 項目が足りないときの値になる
/// ・画面（ViewModel）と同じ形にする必要はない。ViewModel 側の LoadFrom / OnXxxChanged で対応付ける
/// </summary>
public class AppSettings
{
    public GeneralSettings General { get; set; } = new();
    public AppearanceSettings Appearance { get; set; } = new();
}

/// <summary>設定 → 全般</summary>
public class GeneralSettings
{
    public string UserName { get; set; } = "";

    /// <summary>自動保存の設定（入れ子のクラスでひとまとめにして保存する例）</summary>
    public AutoSaveOptions AutoSave { get; set; } = new();
}

/// <summary>
/// 自動保存の設定（「全般」ページの「自動保存する」とその詳細設定）。
/// JSON では "AutoSave": { "Enabled": true, "Interval": 5, "OnClose": true } になる。
/// クラスなので、中の項目は s.General.AutoSave.Interval = 10; のように直接書き換えられる。
/// </summary>
public class AutoSaveOptions
{
    public bool Enabled { get; set; } = true;
    public int Interval { get; set; } = 5;       // 分
    public bool OnClose { get; set; } = true;
}

/// <summary>設定 → 外観</summary>
public class AppearanceSettings
{
    public double FontSize { get; set; } = 12;
}
