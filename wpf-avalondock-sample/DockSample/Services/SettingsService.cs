using System.ComponentModel;
using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using CommunityToolkit.Mvvm.ComponentModel;
using DockSample.Settings;

namespace DockSample.Services;

/// <summary>アプリの設定（AppSettings）の読み込み・保存・元に戻す・別ファイルを開く</summary>
public interface ISettingsService : INotifyPropertyChanged
{
    /// <summary>
    /// 現在の設定（編集中の値）。各 ViewModel はこれを読み書きする。
    /// 「元に戻す」「開く」で丸ごと差し替わるので、中のオブジェクトを変数に覚えておかず、毎回 Current から辿ること。
    /// </summary>
    AppSettings Current { get; }

    /// <summary>
    /// 今開いている設定ファイルのパス（「保存」の保存先）。
    /// まだ一度も保存していない（保存先が決まっていない）ときは null。
    /// </summary>
    string? FilePath { get; }

    /// <summary>最後に保存（または読み込み）してから変更があるか</summary>
    bool IsDirty { get; }

    /// <summary>今のファイル（FilePath）に上書き保存する。保存先が決まっていない・失敗したら false</summary>
    bool Save();

    /// <summary>指定したファイルに保存し、以降はそのファイルを使う。成功したら true</summary>
    bool SaveAs(string path);

    /// <summary>指定した設定ファイルを読み込み、以降はそのファイルを使う。成功したら true</summary>
    bool Open(string path);

    /// <summary>Current を最後に保存（または読み込み）した状態に戻す</summary>
    void Revert();

    /// <summary>Current が変わったことを知らせる（IsDirty を再計算する）</summary>
    void NotifyChanged();

    /// <summary>「元に戻す」「開く」で Current が差し替わった（各 ViewModel は値を読み直す）</summary>
    event EventHandler? Reloaded;
}

/// <summary>
/// 設定を JSON ファイルで保存する。
/// ・「開く」「名前を付けて保存」で使ったファイルは last-settings.txt に覚えておき、次回起動時にそれを開く
/// ・覚えているファイルがなければ、保存先が決まっていない状態（FilePath = null）で初期値から始める
///   （以前の版で使っていた %LocalAppData%\DockSample\settings.json があれば、それを開く）
/// ・最後に保存した内容を JSON 文字列で覚えておき、変更の有無の判定と「元に戻す」に使う
/// </summary>
public partial class JsonSettingsService : ObservableObject, ISettingsService
{
    private static readonly string AppDataDir = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "DockSample");

    /// <summary>以前の版の保存先。「名前を付けて保存」で最初に提案するファイル名にも使う</summary>
    public static readonly string DefaultFilePath = Path.Combine(AppDataDir, "settings.json");

    /// <summary>最後に使った設定ファイルのパスを覚えておくファイル</summary>
    private static readonly string LastFileRecord = Path.Combine(AppDataDir, "last-settings.txt");

    private static readonly JsonSerializerOptions Options = new()
    {
        WriteIndented = true,                            // 人が読める形で保存
        Converters = { new JsonStringEnumConverter() },  // enum を数字ではなく名前で保存
    };

    private readonly ILogService _log;

    /// <summary>最後に保存した（または読み込んだ）内容</summary>
    private string _savedJson;

    public JsonSettingsService(ILogService log)
    {
        _log = log;

        // 前回使ったファイル → 以前の版の既定ファイル → どちらもなければ保存先なし（初期値）
        FilePath = ReadLastFilePath() ?? (File.Exists(DefaultFilePath) ? DefaultFilePath : null);
        Current = FilePath is not null
            ? TryRead(FilePath) ?? new AppSettings()   // 壊れていたら初期値（次の保存で上書きされる）
            : new AppSettings();
        _savedJson = Serialize(Current);
    }

    public AppSettings Current { get; private set; }

    [ObservableProperty] private string? _filePath;
    [ObservableProperty] private bool _isDirty;

    public event EventHandler? Reloaded;

    public void NotifyChanged() => IsDirty = Serialize(Current) != _savedJson;

    public bool Save() => FilePath is not null && WriteTo(FilePath);

    public bool SaveAs(string path)
    {
        if (!WriteTo(path)) return false;
        FilePath = path;
        RememberFilePath(path);
        return true;
    }

    public bool Open(string path)
    {
        var loaded = TryRead(path);
        if (loaded is null) return false;

        Current = loaded;
        FilePath = path;
        RememberFilePath(path);
        _savedJson = Serialize(Current);
        Reloaded?.Invoke(this, EventArgs.Empty);   // 各ページに読み直してもらう
        IsDirty = false;
        return true;
    }

    public void Revert()
    {
        // 最後に保存した内容で丸ごと差し替え、各 ViewModel に読み直してもらう
        Current = JsonSerializer.Deserialize<AppSettings>(_savedJson, Options) ?? new AppSettings();
        Reloaded?.Invoke(this, EventArgs.Empty);
        IsDirty = false;
        _log.Write("設定を元に戻しました");
    }

    // ===== ファイル操作 =====

    private bool WriteTo(string path)
    {
        try
        {
            var json = Serialize(Current);
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, json);
            _savedJson = json;
            IsDirty = false;
            _log.Write($"設定を保存しました: {path}");
            return true;
        }
        catch (Exception ex)
        {
            _log.Write($"設定の保存に失敗: {ex.Message}");   // 失敗したら IsDirty はそのまま
            return false;
        }
    }

    private AppSettings? TryRead(string path)
    {
        try
        {
            var settings = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(path), Options);
            _log.Write($"設定を読み込みました: {path}");
            return settings ?? new AppSettings();
        }
        catch (Exception ex)
        {
            _log.Write($"設定の読み込みに失敗: {path} ({ex.Message})");
            return null;
        }
    }

    /// <summary>前回使った設定ファイル。記録がない・ファイルが消えている場合は null</summary>
    private static string? ReadLastFilePath()
    {
        try
        {
            if (!File.Exists(LastFileRecord)) return null;
            var path = File.ReadAllText(LastFileRecord).Trim();
            return File.Exists(path) ? path : null;
        }
        catch
        {
            return null;
        }
    }

    private void RememberFilePath(string path)
    {
        try
        {
            Directory.CreateDirectory(AppDataDir);
            File.WriteAllText(LastFileRecord, path);
        }
        catch (Exception ex)
        {
            _log.Write($"最後に使った設定ファイルの記録に失敗: {ex.Message}");
        }
    }

    private static string Serialize(AppSettings settings) => JsonSerializer.Serialize(settings, Options);
}
