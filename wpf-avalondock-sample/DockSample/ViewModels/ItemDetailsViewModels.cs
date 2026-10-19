using CommunityToolkit.Mvvm.ComponentModel;
using DockSample.Settings;

namespace DockSample.ViewModels;

/// <summary>
/// 一覧の1行の「詳細設定」の基底。種類の分類ごとに中身（項目）が違う。
/// 右側の詳細パネルには、この型ごとの DataTemplate（ItemListView.xaml）で表示される。
///
/// 保存用のデータ（Settings/ItemSettings.cs の SensorData など）との対応付けは各クラスで手書きする：
///   ・画面 → データ : ToData（保存するとき・「編集したか」を比べるとき）
///   ・データ → 画面 : Load（起動時・Undo・開く で読み直すとき）
/// 項目を追加したら [ObservableProperty]・ToData・Load と、データ側の record の4か所を書くこと。
/// </summary>
public abstract class ItemDetailsBase : ObservableObject
{
    /// <summary>詳細パネルの見出し（「センサの設定」など）</summary>
    public abstract string Title { get; }

    /// <summary>今の値を保存用のデータにする</summary>
    public abstract ItemDetailsData ToData();

    /// <summary>保存用のデータから値を読み込む（型が違うデータなら何もしない＝初期値のまま）</summary>
    protected abstract void Load(ItemDetailsData data);

    /// <summary>種類の分類から、対応する詳細設定を作る。data があればその値を読み込む</summary>
    public static ItemDetailsBase Create(ItemType type, ItemDetailsData? data = null)
    {
        ItemDetailsBase details = type.Category switch
        {
            "センサ"         => new SensorDetails(),
            "アクチュエータ" => new ActuatorDetails(),
            "通信"           => new CommunicationDetails(),
            _                => new GeneralDetails(),
        };
        if (data is not null) details.Load(data);
        return details;
    }
}

/// <summary>センサの設定</summary>
public partial class SensorDetails : ItemDetailsBase
{
    public override string Title => "センサの設定";

    /// <summary>単位の候補（コンボボックスに出す。直接入力も可）</summary>
    public IReadOnlyList<string> Units { get; } = new[] { "℃", "%", "kPa", "lx", "m/s²", "L/min" };

    [ObservableProperty] private int _channel = 1;
    [ObservableProperty] private int _samplingIntervalMs = 1000;
    [ObservableProperty] private string _unit = "℃";
    [ObservableProperty] private double _lowerLimit;
    [ObservableProperty] private double _upperLimit = 100;

    public override ItemDetailsData ToData() => new SensorData
    {
        Channel = Channel, SamplingIntervalMs = SamplingIntervalMs, Unit = Unit,
        LowerLimit = LowerLimit, UpperLimit = UpperLimit,
    };

    protected override void Load(ItemDetailsData data)
    {
        if (data is not SensorData d) return;
        Channel = d.Channel;
        SamplingIntervalMs = d.SamplingIntervalMs;
        Unit = d.Unit;
        LowerLimit = d.LowerLimit;
        UpperLimit = d.UpperLimit;
    }
}

/// <summary>アクチュエータの設定</summary>
public partial class ActuatorDetails : ItemDetailsBase
{
    public override string Title => "アクチュエータの設定";

    [ObservableProperty] private int _channel = 1;
    [ObservableProperty] private double _maxOutputPercent = 100;
    [ObservableProperty] private bool _onAtStartup;

    public override ItemDetailsData ToData() => new ActuatorData
    {
        Channel = Channel, MaxOutputPercent = MaxOutputPercent, OnAtStartup = OnAtStartup,
    };

    protected override void Load(ItemDetailsData data)
    {
        if (data is not ActuatorData d) return;
        Channel = d.Channel;
        MaxOutputPercent = d.MaxOutputPercent;
        OnAtStartup = d.OnAtStartup;
    }
}

/// <summary>通信の設定</summary>
public partial class CommunicationDetails : ItemDetailsBase
{
    public override string Title => "通信の設定";

    [ObservableProperty] private string _host = "localhost";
    [ObservableProperty] private int _port = 1883;
    [ObservableProperty] private int _timeoutMs = 3000;
    [ObservableProperty] private bool _useTls;

    public override ItemDetailsData ToData() => new CommunicationData
    {
        Host = Host, Port = Port, TimeoutMs = TimeoutMs, UseTls = UseTls,
    };

    protected override void Load(ItemDetailsData data)
    {
        if (data is not CommunicationData d) return;
        Host = d.Host;
        Port = d.Port;
        TimeoutMs = d.TimeoutMs;
        UseTls = d.UseTls;
    }
}

/// <summary>その他（入力・出力・制御など）の設定</summary>
public partial class GeneralDetails : ItemDetailsBase
{
    public override string Title => "設定";

    [ObservableProperty] private string _address = "";
    [ObservableProperty] private int _channel = 1;

    public override ItemDetailsData ToData() => new GeneralData { Address = Address, Channel = Channel };

    protected override void Load(ItemDetailsData data)
    {
        if (data is not GeneralData d) return;
        Address = d.Address;
        Channel = d.Channel;
    }
}
