using System.Text.Json;
using System.Text.Json.Serialization;

namespace DockSample.Settings;

/// <summary>
/// waveforms.json 全体。
///
///   {
///     "waveforms": {
///       "PulseGen": {                     ← デバイス名（これで中身の型が決まる。DeviceTypes の表）
///         "1234-abcd": {                  ← uuid
///           "name": "出力1",              ← 表示名（どの機種にもある。DeviceData.Name）
///           "pulseCollection": [ { "name": "パルス0", "segmentCollection": [ ... ] } ]
///         }
///       }
///     }
///   }
///
/// JSON のキーの綴りは [JsonPropertyName] で実際のファイルに合わせる。
/// どのクラスにも [JsonExtensionData] を付けてあるので、ここに書いていない項目も保存時にそのまま書き戻される。
/// </summary>
public class WaveformFile
{
    /// <summary>デバイス名 → uuid → 中身（デバイス名によって型が違う）</summary>
    [JsonPropertyName("waveforms")]
    [JsonConverter(typeof(DeviceTableConverter))]
    public Dictionary<string, Dictionary<string, DeviceData>> Waveforms { get; set; } = new();

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }
}

/// <summary>
/// uuid 1つ分の中身の親（型は DeviceTypes の表で決まるので、ここには型の目印などは付けない）。
/// どの機種も、共通の項目（Name など）はここに書く。派生クラスには自動で付く。
/// </summary>
public abstract class DeviceData
{
    /// <summary>表示名（グラフペインの一覧などに出す）。JSON ではいつも先頭に書く</summary>
    [JsonPropertyName("name"), JsonPropertyOrder(-1)]
    public string Name { get; set; } = "";

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }
}

/// <summary>パルスを持つ機種</summary>
public class WaveformData : DeviceData
{
    /// <summary>画面から新しく作るとき用：パルス1つ（セグメント1つ）を持った状態。読み込みでは使われない</summary>
    public static WaveformData CreateNew() => new() { Pulses = { PulseData.CreateNew() } };

    /// <summary>時間 0 での電圧（V）。最初のセグメントはこの電圧から始まる</summary>
    [JsonPropertyName("restingVoltage")]
    public double RestingVoltage { get; set; }

    [JsonPropertyName("pulseCollection")]
    public List<PulseData> Pulses { get; set; } = new();
}

/// <summary>DC の機種：Hi / Low / Zero の信号を順に並べた矩形波</summary>
public class DcSourceData : DeviceData
{
    /// <summary>画面から新しく作るとき用：信号1つを持った状態。読み込みでは使われない</summary>
    public static DcSourceData CreateNew() => new() { Signals = { DcSignal.CreateNew() } };

    /// <summary>信号の並び（先頭から順に出す）</summary>
    [JsonPropertyName("signals")]
    public List<DcSignal> Signals { get; set; } = new();

    // 以下は最初に仮で作った項目（グラフには使っていない。編集欄で使っている）
    [JsonPropertyName("voltage")] public double Voltage { get; set; }
    [JsonPropertyName("limitCurrent")] public double LimitCurrent { get; set; }
}

/// <summary>
/// DC の信号1つ：レベル（"Hi" / "Low" / "Zero" の文字）と、その長さ（ns）。
/// レベルは文字のまま持つ（知らない文字が入っていても読み込みで失敗せず、そのまま書き戻せるように）。電圧への変換は DcLevel
/// </summary>
public class DcSignal
{
    /// <summary>新しい信号の初期値（Hi・10µs）</summary>
    public static DcSignal CreateNew() => new() { Level = DcLevel.Hi, Duration = TimeUnit.UsToNs(10) };

    [JsonPropertyName("level")]
    public string Level { get; set; } = DcLevel.Zero;

    /// <summary>長さ（ns。画面では µs で見せる）</summary>
    [JsonPropertyName("duration")]
    public double Duration { get; set; }

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }
}

/// <summary>DC の信号のレベル（ファイルに書かれる文字）と電圧</summary>
public static class DcLevel
{
    public const string Hi = "Hi";
    public const string Low = "Low";
    public const string Zero = "Zero";

    /// <summary>レベルの文字 → 電圧（大文字小文字は区別しない。知らない文字は 0V）</summary>
    public static double ToVoltage(string level) => level.Trim().ToLowerInvariant() switch
    {
        "hi" or "high" => 2,
        "low" => -2,
        _ => 0,   // "zero" と、知らない文字
    };
}

/// <summary>表にないデバイス名の中身（項目は全部 Extra に入り、保存時にそのまま書き戻される）</summary>
public class UnknownDeviceData : DeviceData { }

/// <summary>パルス1つ（セグメントの並び）</summary>
public class PulseData
{
    /// <summary>新しいパルス：セグメント1つを持った状態（パルスにはセグメントが必ず1つ以上ある）。名前は並び順で付け直される</summary>
    public static PulseData CreateNew() => new() { Segments = { SegmentData.CreateNew() } };

    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    [JsonPropertyName("segmentCollection")]
    public List<SegmentData> Segments { get; set; } = new();

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }
}

/// <summary>
/// セグメント1つ。単位：TargetVoltage は V、SlewRate は V/µs、Duration は ns（ファイルのまま）。
/// 画面（グラフ・表）では時間を µs で見せる。換算は TimeUnit を使う
/// </summary>
public class SegmentData
{
    /// <summary>新しいセグメントの初期値（5V・スルーレート 1V/µs・10µs）</summary>
    public static SegmentData CreateNew() =>
        new() { Name = "segment", TargetVoltage = 5, SlewRate = 1, Duration = TimeUnit.UsToNs(10) };

    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    [JsonPropertyName("targetVoltage")]
    public double TargetVoltage { get; set; }

    [JsonPropertyName("slewRate")]
    public double SlewRate { get; set; }

    [JsonPropertyName("duration")]
    public double Duration { get; set; }

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }
}

/// <summary>時間の単位の換算（ファイルは ns、画面は µs）</summary>
public static class TimeUnit
{
    /// <summary>ns → µs（画面に見せるとき）</summary>
    public static double NsToUs(double ns) => ns / 1000;

    /// <summary>µs → ns（画面の値をファイルに書くとき。小数の計算誤差は丸める）</summary>
    public static double UsToNs(double us) => Math.Round(us * 1000, 6);
}

/// <summary>デバイス名から、中身のクラスを決める。機種を増やしたらここに1行足す</summary>
public static class DeviceTypes
{
    /// <summary>デバイス名 → 中身の型と、新しく作るときの作り方（初期の中身入り）</summary>
    private static readonly Dictionary<string, (Type Type, Func<DeviceData> CreateNew)> Table = new()
    {
        ["PulseGen"] = (typeof(WaveformData), WaveformData.CreateNew),
        ["DcSource"] = (typeof(DcSourceData), DcSourceData.CreateNew),
    };

    /// <summary>表にあるデバイス名（グラフペインの「追加 ▾」の一覧に出す）</summary>
    public static IReadOnlyCollection<string> Names => Table.Keys;

    /// <summary>このデバイス名の中身の型（表にない名前なら UnknownDeviceData＝中身をそのまま保持）</summary>
    public static Type For(string deviceName) =>
        Table.TryGetValue(deviceName, out var entry) ? entry.Type : typeof(UnknownDeviceData);

    /// <summary>
    /// このデバイス名用の中身を新しく作る（初期の中身入り：パルス1つ、信号1つ など）。新しく uuid を足すときは必ずこれで作る。
    /// JSON からの読み込みはこれを通らない（ファイルに無い中身が増えることはない）
    /// </summary>
    public static DeviceData Create(string deviceName) =>
        Table.TryGetValue(deviceName, out var entry) ? entry.CreateNew() : new UnknownDeviceData();
}

/// <summary>
/// "waveforms" の中身（デバイス名 → uuid → 中身）を読み書きする。
/// 読むとき：デバイス名から型を決めて、その下の uuid の中身を全部その型で読む。
/// 書くとき：それぞれ実際の型で書く。
/// </summary>
public class DeviceTableConverter : JsonConverter<Dictionary<string, Dictionary<string, DeviceData>>>
{
    public override Dictionary<string, Dictionary<string, DeviceData>> Read(
        ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        using var doc = JsonDocument.ParseValue(ref reader);
        var result = new Dictionary<string, Dictionary<string, DeviceData>>();

        foreach (var device in doc.RootElement.EnumerateObject())      // デバイスごと
        {
            var type = DeviceTypes.For(device.Name);                   // ← デバイス名で型を決める
            var byUuid = new Dictionary<string, DeviceData>();

            foreach (var uuid in device.Value.EnumerateObject())       // uuid ごと
                byUuid[uuid.Name] = (DeviceData?)uuid.Value.Deserialize(type, options)
                                    ?? DeviceTypes.Create(device.Name);

            result[device.Name] = byUuid;
        }
        return result;
    }

    public override void Write(Utf8JsonWriter writer,
        Dictionary<string, Dictionary<string, DeviceData>> value, JsonSerializerOptions options)
    {
        writer.WriteStartObject();
        foreach (var (device, byUuid) in value)
        {
            writer.WritePropertyName(device);
            writer.WriteStartObject();
            foreach (var (uuid, data) in byUuid)
            {
                writer.WritePropertyName(uuid);
                JsonSerializer.Serialize(writer, data, data.GetType(), options);   // 実際の型で書く
            }
            writer.WriteEndObject();
        }
        writer.WriteEndObject();
    }
}
