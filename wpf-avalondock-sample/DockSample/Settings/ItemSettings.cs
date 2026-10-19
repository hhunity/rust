using System.Text.Json.Serialization;

namespace DockSample.Settings;

/// <summary>
/// 「一覧」ペインの1行分（items.json に保存されるデータ）。
/// 画面用の ItemRowViewModel とは別の、ただのデータ。
/// </summary>
public class ItemData
{
    /// <summary>行を見分けるための番号（名前は変えられるので、保存した行との対応はこれで取る）</summary>
    public Guid Id { get; set; } = Guid.NewGuid();

    /// <summary>種類（ItemType の Name と Category）</summary>
    public string TypeName { get; set; } = "";
    public string Category { get; set; } = "";

    public string Name { get; set; } = "";
    public string Notes { get; set; } = "";
    public bool Enabled { get; set; } = true;

    /// <summary>種類ごとの詳細設定。中身の型は JSON の "kind" で見分ける</summary>
    public ItemDetailsData? Details { get; set; }
}

/// <summary>
/// 種類ごとの詳細設定の基底。派生クラスによって中身が違うので、JSON には
///   "Details": { "kind": "sensor", "Channel": 1, ... }
/// のように型の目印（kind）を付けて保存し、読み込むときはそれを見て型を選ぶ。
/// 詳細設定の種類を増やしたら、ここに [JsonDerivedType] を1行足す。
///
/// record なので == で中身どうしを比べられる（「保存したときから変わったか」の判定に使う）。
/// </summary>
[JsonPolymorphic(TypeDiscriminatorPropertyName = "kind")]
[JsonDerivedType(typeof(SensorData), "sensor")]
[JsonDerivedType(typeof(ActuatorData), "actuator")]
[JsonDerivedType(typeof(CommunicationData), "communication")]
[JsonDerivedType(typeof(GeneralData), "general")]
public abstract record ItemDetailsData;

public record SensorData : ItemDetailsData
{
    public int Channel { get; init; } = 1;
    public int SamplingIntervalMs { get; init; } = 1000;
    public string Unit { get; init; } = "℃";
    public double LowerLimit { get; init; }
    public double UpperLimit { get; init; } = 100;
}

public record ActuatorData : ItemDetailsData
{
    public int Channel { get; init; } = 1;
    public double MaxOutputPercent { get; init; } = 100;
    public bool OnAtStartup { get; init; }
}

public record CommunicationData : ItemDetailsData
{
    public string Host { get; init; } = "localhost";
    public int Port { get; init; } = 1883;
    public int TimeoutMs { get; init; } = 3000;
    public bool UseTls { get; init; }
}

public record GeneralData : ItemDetailsData
{
    public string Address { get; init; } = "";
    public int Channel { get; init; } = 1;
}
