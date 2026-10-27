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

/// <summary>uuid 1つ分の中身の親（型は DeviceTypes の表で決まるので、ここには何も付けない）</summary>
public abstract class DeviceData
{
    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }
}

/// <summary>パルスを持つ機種</summary>
public class WaveformData : DeviceData
{
    [JsonPropertyName("pulseCollection")]
    public List<PulseData> Pulses { get; set; } = new();
}

/// <summary>別の機種の例（一定電圧を出すだけ）</summary>
public class DcSourceData : DeviceData
{
    [JsonPropertyName("voltage")] public double Voltage { get; set; }
    [JsonPropertyName("limitCurrent")] public double LimitCurrent { get; set; }
}

/// <summary>表にないデバイス名の中身（項目は全部 Extra に入り、保存時にそのまま書き戻される）</summary>
public class UnknownDeviceData : DeviceData { }

/// <summary>パルス1つ（セグメントの並び）</summary>
public class PulseData
{
    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    [JsonPropertyName("segmentCollection")]
    public List<SegmentData> Segments { get; set; } = new();

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }
}

/// <summary>セグメント1つ</summary>
public class SegmentData
{
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

/// <summary>デバイス名から、中身のクラスを決める。機種を増やしたらここに1行足す</summary>
public static class DeviceTypes
{
    private static readonly Dictionary<string, Type> Table = new()
    {
        ["PulseGen"] = typeof(WaveformData),
        ["DcSource"] = typeof(DcSourceData),
    };

    /// <summary>このデバイス名の中身の型（表にない名前なら UnknownDeviceData＝中身をそのまま保持）</summary>
    public static Type For(string deviceName) =>
        Table.GetValueOrDefault(deviceName, typeof(UnknownDeviceData));

    /// <summary>このデバイス名用の、空の中身を作る（新しく uuid を足すときは必ずこれで作る）</summary>
    public static DeviceData Create(string deviceName) =>
        (DeviceData)Activator.CreateInstance(For(deviceName))!;
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
