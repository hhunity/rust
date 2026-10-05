using System.ComponentModel;
using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using CommunityToolkit.Mvvm.ComponentModel;
using DockSample.Settings;

namespace DockSample.Services;

/// <summary>アプリの設定（AppSettings）の読み込み・保存・元に戻す</summary>
public interface ISettingsService : INotifyPropertyChanged
{
    /// <summary>現在の設定（編集中の値）。各 ViewModel はこれを読み書きする</summary>
    AppSettings Current { get; }

    /// <summary>最後に保存（または起動時に読み込み）してから変更があるか</summary>
    bool IsDirty { get; }

    /// <summary>Current を settings.json に保存する</summary>
    void Save();

    /// <summary>Current を最後に保存した状態に戻し、Reverted を通知する</summary>
    void Revert();

    /// <summary>Current が変わったことを知らせる（IsDirty を再計算する）</summary>
    void NotifyChanged();

    /// <summary>Revert で Current が書き戻された（各 ViewModel は値を読み直す）</summary>
    event EventHandler? Reverted;
}

/// <summary>
/// %LocalAppData%\DockSample\settings.json に JSON で保存する。
/// （ファイルを消すと初期値に戻る）
/// 最後に保存した内容を JSON 文字列で覚えておき、変更の有無の判定と「元に戻す」に使う。
/// </summary>
public partial class JsonSettingsService : ObservableObject, ISettingsService
{
    private static readonly string FilePath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "DockSample", "settings.json");

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
        Current = Load();
        _savedJson = Serialize(Current);
    }

    public AppSettings Current { get; }

    [ObservableProperty] private bool _isDirty;

    public event EventHandler? Reverted;

    public void NotifyChanged() => IsDirty = Serialize(Current) != _savedJson;

    public void Save()
    {
        try
        {
            var json = Serialize(Current);
            Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
            File.WriteAllText(FilePath, json);
            _savedJson = json;
            IsDirty = false;
            _log.Write("設定を保存しました");
        }
        catch (Exception ex)
        {
            _log.Write($"設定の保存に失敗: {ex.Message}");   // 失敗したら IsDirty は true のまま
        }
    }

    public void Revert()
    {
        var saved = JsonSerializer.Deserialize<AppSettings>(_savedJson, Options) ?? new AppSettings();

        // Current のオブジェクトは各 ViewModel が参照しているので、差し替えずに中身だけ書き戻す
        foreach (var section in typeof(AppSettings).GetProperties())
        {
            if (section.GetValue(saved) is { } from && section.GetValue(Current) is { } to)
                PropertyCopier.CopyAll(from, to);
        }

        Reverted?.Invoke(this, EventArgs.Empty);
        IsDirty = false;
        _log.Write("設定を元に戻しました");
    }

    private AppSettings Load()
    {
        try
        {
            if (!File.Exists(FilePath)) return new AppSettings();
            var settings = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(FilePath), Options);
            _log.Write("設定を読み込みました");
            return settings ?? new AppSettings();
        }
        catch (Exception ex)
        {
            // 壊れていたら初期値で起動する（ファイルは次の保存で上書きされる）
            _log.Write($"設定の読み込みに失敗したため初期値を使います: {ex.Message}");
            return new AppSettings();
        }
    }

    private static string Serialize(AppSettings settings) => JsonSerializer.Serialize(settings, Options);
}
