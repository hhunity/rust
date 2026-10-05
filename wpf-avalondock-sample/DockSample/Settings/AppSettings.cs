namespace DockSample.Settings;

/// <summary>
/// settings.json に保存する設定（ただのデータ）。
/// ・ここに書いた初期値が、設定ファイルがまだないとき / 項目が足りないときの値になる
/// ・プロパティ名は、読み書きする ViewModel のプロパティ名と完全に一致させること
///   （PropertyCopier が名前でコピーする。名前が違うと黙って保存されない）
/// </summary>
public class AppSettings
{
    public GeneralSettings General { get; set; } = new();
    public AppearanceSettings Appearance { get; set; } = new();
}

/// <summary>設定 → 全般（GeneralPageViewModel と同じ名前）</summary>
public class GeneralSettings
{
    public string UserName { get; set; } = "";
    public bool AutoSave { get; set; } = true;
    public int AutoSaveInterval { get; set; } = 5;
    public bool SaveOnClose { get; set; } = true;
}

/// <summary>設定 → 外観（AppearancePageViewModel と同じ名前）</summary>
public class AppearanceSettings
{
    public double FontSize { get; set; } = 12;
}
