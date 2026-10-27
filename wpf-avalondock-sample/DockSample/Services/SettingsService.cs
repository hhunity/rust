using System.ComponentModel;
using System.IO;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using CommunityToolkit.Mvvm.ComponentModel;
using DockSample.Settings;

namespace DockSample.Services;

/// <summary>アプリの設定（AppSettings）の読み込み・保存・別ファイルを開く・Undo/Redo</summary>
public interface ISettingsService : INotifyPropertyChanged
{
    /// <summary>
    /// 現在の設定（編集中の値）。各 ViewModel はこれを読み書きする。
    /// 「元に戻す」「開く」で丸ごと差し替わるので、中のオブジェクトを変数に覚えておかず、毎回 Current から辿ること。
    /// </summary>
    AppSettings Current { get; }

    /// <summary>
    /// 今開いている設定フォルダのパス（「保存」の保存先）。中にセクションごとの JSON が入る。
    /// まだ一度も保存していない（保存先が決まっていない）ときは null。
    /// </summary>
    string? FolderPath { get; }

    /// <summary>最後に保存（または読み込み）してから変更があるか</summary>
    bool IsDirty { get; }

    /// <summary>今のフォルダ（FolderPath）に上書き保存する。保存先が決まっていない・失敗したら false</summary>
    bool Save();

    /// <summary>指定したフォルダに保存し（なければ作る）、以降はそのフォルダを使う。成功したら true</summary>
    bool SaveAs(string folder);

    /// <summary>
    /// 指定したフォルダの設定を読み込み、以降はそのフォルダを使う。
    /// 設定ファイルが1つもない・読めないファイルがあるときは false（今の設定はそのまま）
    /// </summary>
    bool Open(string folder);

    /// <summary>フォルダに、このアプリの設定ファイルが1つでもあるか（上書き確認に使う）</summary>
    bool HasSettingsFiles(string folder);

    /// <summary>Current を最後に保存（または読み込み）した状態に戻す（これも Undo で取り消せる）</summary>
    void Revert();

    /// <summary>1つ前の状態に戻せるか / やり直せるか</summary>
    bool CanUndo { get; }
    bool CanRedo { get; }

    /// <summary>
    /// Current を書き換える。書き換える前の状態を Undo 用に記録してから edit を呼び、「未保存の変更」を更新する。
    /// ・同じ key の変更が短い間に続いた場合（文字入力・数値の連続増減など）は、1回分の Undo にまとめる
    /// ・Reloaded の最中（各 ViewModel が読み直した値を自分のプロパティに入れているだけ）に来た Edit は無視する
    /// </summary>
    void Edit(string key, Action<AppSettings> edit);

    /// <summary>1つ前の状態に戻す / Undo で戻した分をやり直す</summary>
    void Undo();
    void Redo();

    /// <summary>「保存した状態に戻す」「開く」「Undo/Redo」で Current が差し替わった（各 ViewModel は値を読み直す）</summary>
    event EventHandler? Reloaded;

    /// <summary>保存に成功した（「保存した状態」が今の内容になった）</summary>
    event EventHandler? Saved;

    /// <summary>最後に保存（または読み込み）した状態のコピー（「新規」「編集済み」の判定に使う）</summary>
    AppSettings GetSaved();
}

/// <summary>
/// 設定を「フォルダ＋セクションごとの JSON ファイル」で保存する。
///
///   MyProject/
///   ├─ general.json      ← AppSettings.General
///   ├─ appearance.json   ← AppSettings.Appearance
///   ├─ items.json        ← AppSettings.Items（「一覧」ペインの行）
///   └─ waveforms.json    ← AppSettings.Waveforms（波形）
///
/// ・どのセクションをどのファイルに入れるかは Sections の一覧で決める（増やすときは1行足す）
/// ・読み込み時、フォルダにないファイルのセクションは初期値になる（後からセクションを増やしても古いフォルダを開ける）
/// ・メモリ上では1つの AppSettings として扱う（Undo・変更の有無の判定は設定全体の JSON で行う）
/// ・「開く」「名前を付けて保存」で使ったフォルダは last-folder.txt に覚えておき、次回起動時にそれを開く
/// ・覚えているフォルダがなければ、保存先が決まっていない状態（FolderPath = null）で初期値から始める
///   （以前の版の %LocalAppData%\DockSample\settings.json があれば、その内容を読み込む。保存先は未定のまま）
/// </summary>
public partial class JsonSettingsService : ObservableObject, ISettingsService
{
    private static readonly string AppDataDir = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "DockSample");

    /// <summary>以前の版の保存先（1ファイルに全部入り）。見つかれば起動時に内容だけ読み込む</summary>
    private static readonly string LegacyFilePath = Path.Combine(AppDataDir, "settings.json");

    /// <summary>最後に使った設定フォルダのパスを覚えておくファイル</summary>
    private static readonly string LastFolderRecord = Path.Combine(AppDataDir, "last-folder.txt");

    private static readonly JsonSerializerOptions Options = new()
    {
        WriteIndented = true,                            // 人が読める形で保存
        Converters = { new JsonStringEnumConverter() },  // enum を数字ではなく名前で保存
        PropertyNameCaseInsensitive = true,                          // キーの大文字小文字の違いを許す
        NumberHandling = JsonNumberHandling.AllowReadingFromString,  // "5" のような文字列の数値も読める
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,       // 日本語を \uXXXX にせず保存
    };

    /// <summary>設定ファイルと同じ JSON の書き方（波形のエクスポート・インポートなど、設定の一部を別ファイルに読み書きするとき用）</summary>
    public static JsonSerializerOptions JsonOptions => Options;

    // ===== セクション ⇔ ファイル の対応 =====

    /// <summary>1ファイル分：ファイル名と、そのセクションの書き出し・読み込み方</summary>
    private sealed record Section(
        string FileName,
        Func<AppSettings, string> Write,
        Action<AppSettings, string> Read);

    private static Section Map<T>(string fileName, Func<AppSettings, T> get, Action<AppSettings, T> set)
        where T : new() =>
        new(fileName,
            s => JsonSerializer.Serialize(get(s), Options),
            (s, json) => set(s, JsonSerializer.Deserialize<T>(json, Options) ?? new T()));

    /// <summary>
    /// フォルダに保存するファイルの一覧。セクションを増やしたら、ここに1行足す。
    /// </summary>
    private static readonly Section[] Sections =
    {
        Map("general.json",    s => s.General,    (s, v) => s.General = v),
        Map("appearance.json", s => s.Appearance, (s, v) => s.Appearance = v),
        Map("items.json",      s => s.Items,      (s, v) => s.Items = v),
        Map("waveforms.json",  s => s.Waveforms,  (s, v) => s.Waveforms = v),
    };

    private readonly ILogService _log;

    /// <summary>最後に保存した（または読み込んだ）内容</summary>
    private string _savedJson;

    // ===== Undo / Redo =====
    // 変更のたびに「変更前の設定全体」を JSON にして積む。設定は小さいので丸ごと記録で十分。

    /// <summary>同じ項目の変更をこの時間内なら1回分にまとめる</summary>
    private static readonly TimeSpan MergeWindow = TimeSpan.FromSeconds(1.5);

    /// <summary>Undo / Redo で戻れる回数の上限（1回分 = 設定全体の JSON 1つ。古いものから捨てる）</summary>
    private const int MaxHistory = 50;

    private readonly HistoryStack<string> _undo = new(MaxHistory);
    private readonly HistoryStack<string> _redo = new(MaxHistory);
    private string? _lastKey;
    private DateTime _lastTime;

    public JsonSettingsService(ILogService log)
    {
        _log = log;

        // 前回使ったフォルダ → なければ保存先なし（以前の版のファイルがあれば内容だけ読み込む）
        FolderPath = ReadLastFolder();
        Current = FolderPath is not null
            ? TryReadFolder(FolderPath) ?? new AppSettings()   // 読めなければ初期値（次の保存で上書きされる）
            : TryReadLegacyFile() ?? new AppSettings();
        _savedJson = Serialize(Current);
    }

    public AppSettings Current { get; private set; }

    [ObservableProperty] private string? _folderPath;
    [ObservableProperty] private bool _isDirty;
    [ObservableProperty] private bool _canUndo;
    [ObservableProperty] private bool _canRedo;

    public event EventHandler? Reloaded;
    public event EventHandler? Saved;

    public AppSettings GetSaved() =>
        JsonSerializer.Deserialize<AppSettings>(_savedJson, Options) ?? new AppSettings();

    /// <summary>Reloaded を出している最中か（このあいだの Edit は無視する）</summary>
    private bool _reloading;

    public void Edit(string key, Action<AppSettings> edit)
    {
        if (_reloading) return;   // 読み直しで値を入れているだけなので、書き戻さない
        RecordUndo(key);
        edit(Current);
        NotifyChanged();
    }

    /// <summary>Reloaded を出す。各 ViewModel が読み直している間に来た Edit は無視する</summary>
    private void RaiseReloaded()
    {
        _reloading = true;
        try { Reloaded?.Invoke(this, EventArgs.Empty); }
        finally { _reloading = false; }
    }

    /// <summary>Current が変わったあとに呼ぶ（IsDirty を再計算する）</summary>
    private void NotifyChanged()
    {
        var json = Serialize(Current);
        IsDirty = json != _savedJson;

        // 変更した結果が直前の Undo 記録と同じ（例：チェックを付けてすぐ外した）なら、
        // 戻しても何も変わらない記録なので捨てる
        if (_undo.TryPeek(out var top) && top == json)
        {
            _undo.TryPop(out _);
            _lastKey = null;
            UpdateUndoState();
        }
    }

    /// <summary>
    /// 変更の直前に呼ぶ。今の状態を Undo 用に記録する。
    /// 同じ key の変更が短い間に続いた場合は、1回分にまとめる。
    /// </summary>
    private void RecordUndo(string key)
    {
        var now = DateTime.Now;
        if (key == _lastKey && now - _lastTime < MergeWindow)
        {
            _lastTime = now;   // 続けての変更：1回分にまとめる（記録しない）
            return;
        }

        PushUndo();
        _lastKey = key;
        _lastTime = now;
    }

    public void Undo()
    {
        if (!_undo.TryPop(out var previous)) return;
        _redo.Push(Serialize(Current));
        Restore(previous);
    }

    public void Redo()
    {
        if (!_redo.TryPop(out var next)) return;
        _undo.Push(Serialize(Current));
        Restore(next);
    }

    /// <summary>今の状態を Undo 用に積む（新しい変更をしたら、やり直しの履歴は消える）</summary>
    private void PushUndo()
    {
        _undo.Push(Serialize(Current));
        _redo.Clear();
        UpdateUndoState();
    }

    /// <summary>記録しておいた状態に差し替え、各ページに読み直してもらう</summary>
    private void Restore(string json)
    {
        Current = JsonSerializer.Deserialize<AppSettings>(json, Options) ?? new AppSettings();
        _lastKey = null;   // 戻した直後の変更は、新しい1回分として記録する
        RaiseReloaded();
        IsDirty = Serialize(Current) != _savedJson;   // 保存した状態まで戻れば「未保存」は消える
        UpdateUndoState();
    }

    private void ClearHistory()
    {
        _undo.Clear();
        _redo.Clear();
        _lastKey = null;
        UpdateUndoState();
    }

    private void UpdateUndoState()
    {
        CanUndo = _undo.Count > 0;
        CanRedo = _redo.Count > 0;
    }

    public bool Save() => FolderPath is not null && WriteFolder(FolderPath);

    public bool SaveAs(string folder)
    {
        if (!WriteFolder(folder)) return false;
        FolderPath = folder;
        RememberFolder(folder);
        return true;
    }

    public bool Open(string folder)
    {
        var loaded = TryReadFolder(folder);
        if (loaded is null) return false;

        Current = loaded;
        FolderPath = folder;
        RememberFolder(folder);
        _savedJson = Serialize(Current);
        ClearHistory();                            // 別のフォルダの状態には戻れないようにする
        RaiseReloaded();                           // 各ページに読み直してもらう
        IsDirty = false;
        return true;
    }

    public void Revert()
    {
        // これも1回の変更として記録し、Undo で取り消せるようにする
        PushUndo();

        // 最後に保存した内容で丸ごと差し替え、各 ViewModel に読み直してもらう
        Restore(_savedJson);
        _log.Write("設定を保存した状態に戻しました");
    }

    // ===== フォルダ・ファイル操作 =====

    public bool HasSettingsFiles(string folder) =>
        Directory.Exists(folder) && Sections.Any(sec => File.Exists(Path.Combine(folder, sec.FileName)));

    private bool WriteFolder(string folder)
    {
        try
        {
            Directory.CreateDirectory(folder);
            foreach (var sec in Sections)
                File.WriteAllText(Path.Combine(folder, sec.FileName), sec.Write(Current));

            _savedJson = Serialize(Current);
            IsDirty = false;
            Saved?.Invoke(this, EventArgs.Empty);
            _log.Write($"設定を保存しました: {folder}");
            return true;
        }
        catch (Exception ex)
        {
            _log.Write($"設定の保存に失敗: {ex.Message}");   // 失敗したら IsDirty はそのまま
            return false;
        }
    }

    /// <summary>
    /// フォルダの中のファイルを読み、1つの AppSettings にまとめる。
    /// ないファイルのセクションは初期値。設定ファイルが1つもない・読めないファイルがあれば null。
    /// </summary>
    private AppSettings? TryReadFolder(string folder)
    {
        try
        {
            var settings = new AppSettings();
            var found = 0;
            foreach (var sec in Sections)
            {
                var path = Path.Combine(folder, sec.FileName);
                if (!File.Exists(path)) continue;   // このセクションは初期値のまま
                sec.Read(settings, File.ReadAllText(path));
                found++;
            }

            if (found == 0)
            {
                _log.Write($"設定ファイルが見つかりません: {folder}");
                return null;
            }

            _log.Write($"設定を読み込みました: {folder}（{found}/{Sections.Length} ファイル）");
            return settings;
        }
        catch (Exception ex)
        {
            _log.Write($"設定の読み込みに失敗: {folder} ({ex.Message})");
            return null;
        }
    }

    /// <summary>以前の版の1ファイル形式の設定。なければ・読めなければ null</summary>
    private AppSettings? TryReadLegacyFile()
    {
        try
        {
            if (!File.Exists(LegacyFilePath)) return null;
            var settings = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(LegacyFilePath), Options);
            _log.Write("以前の版の設定を読み込みました（保存するとフォルダ形式で保存されます）");
            return settings;
        }
        catch
        {
            return null;
        }
    }

    /// <summary>前回使った設定フォルダ。記録がない・フォルダが消えている場合は null</summary>
    private static string? ReadLastFolder()
    {
        try
        {
            if (!File.Exists(LastFolderRecord)) return null;
            var folder = File.ReadAllText(LastFolderRecord).Trim();
            return Directory.Exists(folder) ? folder : null;
        }
        catch
        {
            return null;
        }
    }

    private void RememberFolder(string folder)
    {
        try
        {
            Directory.CreateDirectory(AppDataDir);
            File.WriteAllText(LastFolderRecord, folder);
        }
        catch (Exception ex)
        {
            _log.Write($"最後に使った設定フォルダの記録に失敗: {ex.Message}");
        }
    }

    private static string Serialize(AppSettings settings) => JsonSerializer.Serialize(settings, Options);
}
