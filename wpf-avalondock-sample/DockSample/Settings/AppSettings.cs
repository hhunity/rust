namespace DockSample.Settings;

/// <summary>
/// settings.json に保存する設定（ただのデータ）。
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

    /// <summary>自動保存の設定（構造体でひとまとめにして保存する例）</summary>
    public AutoSaveOptions AutoSave { get; set; } = new();
}

/// <summary>
/// 自動保存の設定。JSON では "AutoSave": { "Enabled": true, "Interval": 5, "OnClose": true } になる。
/// 構造体なので中の項目は直接書き換えられない。変えるときは with で作り直す：
///   s.AutoSave = s.AutoSave with { Interval = 10 };
/// </summary>
public record struct AutoSaveOptions
{
    // 初期値付きのプロパティを持つ構造体には、引数なしのコンストラクタを明示する。
    // （JSON の読み込みもこれを使うので、ファイルにない項目は下の初期値になる）
    public AutoSaveOptions() { }

    public bool Enabled { get; init; } = true;
    public int Interval { get; init; } = 5;      // 分
    public bool OnClose { get; init; } = true;
}

/// <summary>設定 → 外観</summary>
public class AppearanceSettings
{
    public double FontSize { get; set; } = 12;
}
