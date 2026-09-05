// io: 標準入力(キーボード入力)や、TCP通信のバイト列を読み書きするための機能
// BufRead: 「1行ずつ読む」ためのメソッド(.lines())を使えるようにするトレイト
// Read/Write: TcpStreamからバイト列を読み書きするためのトレイト
use std::io::{self, BufRead, Read, Write};
// TcpListener: 「ここに繋いできて」と待ち受けるための機能（サーバー役）
// TcpStream: 実際にTCPで繋がった通信路（データを送受信する）
// UdpSocket: 自分のIPアドレスを調べるための小技に使う（下のdetect_local_ip関数を参照）
use std::net::{TcpListener, TcpStream, UdpSocket};
// ファイル読み書き(open/createなど)のための機能
use std::fs;
// ファイルパスを表す型
use std::path::{Path, PathBuf};
// 複数スレッドから安全に共有するための「Arc（参照カウント付きの共有ポインタ）」と
// 「Mutex（同時に1つのスレッドしか中身を触れないようにする鍵）」
use std::sync::{mpsc, Arc, Mutex};
// HashMap: 名前→オンライン状況、のような辞書。HashSet: 「まだ返事が来ていない名前の集合」に使う
use std::collections::{HashMap, HashSet};
// thread: 別スレッド（並行して動く処理の流れ）を作るための機能
use std::thread;
// Duration: 「5秒」のような時間の長さを表す型
// Instant: 「今からどれだけ時間が経ったか」を測るための時刻
// SystemTime/UNIX_EPOCH: 「今の時刻」を使って、送信申し出ごとの重複しないIDを作るために使う
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// rumqttcクレート（外部ライブラリ）が提供している、MQTTクライアントを作るための部品たち
// LastWill: 接続時に登録しておく「もし異常切断したら、この内容を代わりにpublishして」という遺言メッセージ
use rumqttc::{Client, Event, LastWill, MqttOptions, Packet, QoS};
// serdeの Serialize/Deserialize を derive すると、構造体を自動でJSON文字列に変換したり、
// JSON文字列から構造体に読み戻したりできるようになる
use serde::{Deserialize, Serialize};

/// `<topic>/file/offer/<宛先名>` のペイロード。「このファイルを送りたい」という申し出。
///
/// seqは、Sparkplug B（前回説明した産業IoT向けMQTT規約）の考え方を参考にした連番。
/// このクライアントが何かをpublishするたびに1ずつ増える値で、受信側はこれを見て
/// 「間の1通が届いていない（抜けている）」ことに気付けるようにする。
#[derive(Serialize, Deserialize)]
struct OfferMsg {
    id: String,
    from: String,
    to: String,
    filename: String,
    size: u64,
    seq: u64,
}

/// `<topic>/file/ack/<宛先名>` のペイロード。「ここ(host:port)に繋いで」という返事。
#[derive(Serialize, Deserialize)]
struct AckMsg {
    id: String,
    /// この返事を送っている（＝ファイルを受け取る）側の名前
    from: String,
    host: String,
    port: u16,
    seq: u64,
}

/// `<topic>/file/received/<マイコン名>` のペイロード。生TCP転送が終わった後の結果報告。
#[derive(Serialize, Deserialize)]
struct ReceivedMsg {
    id: String,
    who: String,
    /// "ok" か "failed"
    status: String,
    size: u64,
    seq: u64,
}

/// `<topic>/presence/<名前>` のペイロード。
#[derive(Serialize, Deserialize)]
struct PresenceMsg {
    /// "online" か "offline"
    status: String,
    seq: u64,
}

/// `<topic>/job/queue` のペイロード。全マイコンへの一斉配信ジョブ。
#[derive(Serialize, Deserialize)]
struct JobMsg {
    id: String,
    /// このジョブを配信した（＝パソコン役の）名前
    from: String,
    content: String,
    seq: u64,
}

/// `<topic>/job/done/<マイコン名>` のペイロード。ジョブの完了報告。
#[derive(Serialize, Deserialize)]
struct DoneMsg {
    id: String,
    who: String,
    seq: u64,
}

/// 送信申し出(id)ごとに「これから送るファイルのパス」を覚えておく辞書。
/// 標準入力を読むスレッドと、MQTT受信を処理するメインスレッドの両方から触るので、
/// Arc<Mutex<...>> で「複数スレッドから安全に共有できる箱」にしている。
type PendingOffers = Arc<Mutex<HashMap<String, PathBuf>>>;

/// 「マイコンの名前 → 今オンラインかどうか」を覚えておく辞書（ジョブ配信先の名簿）。
/// presenceトピックへの通知が来るたびに更新される。
type Roster = Arc<Mutex<HashMap<String, bool>>>;

/// 今まさに配信中で、全員の完了報告を待っているジョブの情報。
/// tx（送信側）はMQTT受信を処理するメインスレッドが持ち、「このマイコンから完了報告が来たよ」と
/// 通知するのに使う。rx（受信側）は/jobコマンドを実行した標準入力スレッドが持ち、
/// 通知が来るたびにチェックし、全員分揃うかタイムアウトするまで待つ。
struct InFlightJob {
    id: String,
    tx: mpsc::Sender<String>,
}
type InFlightState = Arc<Mutex<Option<InFlightJob>>>;

/// ジョブを送ってから、完了報告が来ないマイコンを「エラー」と判断するまでの待ち時間
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

/// 次に使うseq番号を発行するためのカウンタ。
///
/// 重要: **メッセージの種類（OFFERかJOBかなど）ごとに別々のSeqCounterを用意する**必要がある。
/// 理由は、例えば`ACK`は「申し出た本人だけ」に届く専用トピックで、他の観測者には
/// そもそも見えないメッセージだから。もし全種類で1つの共有カウンタを使うと、
/// ACKの分だけ他の観測者から見て番号が「飛んで」見えてしまい、実際は何も欠落していないのに
/// 誤って警告が出てしまう（実際にこの実装で一度その誤検知が起きた）。
/// あるメッセージ種類を購読している人には、その種類のメッセージは必ず全部見えるはずなので、
/// 種類ごとに分ければ正しく欠落を検知できる。
type SeqCounter = Arc<Mutex<u64>>;

/// 相手の名前ごとに「最後に見たseq番号」を覚えておく辞書。次に来たメッセージのseqと比べて、
/// 1つ飛んでいたら「間の1通が抜けたかもしれない」と分かる。
/// SeqCounterと同様、**メッセージの種類ごとに別々のSeqTrackerを使う**。
type SeqTracker = Arc<Mutex<HashMap<String, u64>>>;

/// SeqCounterから「次に使うseq番号」を1つ取り出し、内部のカウンタを1つ進める。
fn next_seq(counter: &SeqCounter) -> u64 {
    let mut n = counter.lock().unwrap();
    let seq = *n;
    *n += 1;
    seq
}

/// 受信したメッセージのseq番号を確認し、直前に見た値から1つも増えていなければ
/// （＝間の番号が抜けていれば）警告を表示する。呼び出す側は、メッセージの種類に対応する
/// 専用のSeqTrackerを渡すこと（他の種類のトラッカーと混ぜて使わない）。
///
/// - 初めて見る名前の場合は比較のしようがないので、警告なしでそのまま記録するだけにする
/// - is_birthがtrueのとき（presenceの"online"、Sparkplug BでいうBIRTH）は、再接続で
///   カウンタが0から数え直されているのが正常なので、ここでも警告なしで記録し直す
fn check_seq(from: &str, seq: u64, tracker: &SeqTracker, is_birth: bool) {
    let mut last_seen = tracker.lock().unwrap();
    if !is_birth {
        if let Some(&last) = last_seen.get(from) {
            if seq != last + 1 {
                println!(
                    "[system] 警告: {from}からのメッセージが抜けている可能性があります\
                     （前回seq={last}, 今回seq={seq}）"
                );
            }
        }
    }
    last_seen.insert(from.to_string(), seq);
}

/// メッセージ種類ごとのSeqCounter/SeqTrackerを1つにまとめた箱。
/// 中身は全部Arc（参照カウント付きの共有ポインタ）なので、cloneしても中身は複製されず、
/// 同じカウンタ・同じトラッカーを指したまま増える（スレッド間で共有するのに丁度いい）。
#[derive(Clone)]
struct SeqState {
    offer_counter: SeqCounter,
    offer_tracker: SeqTracker,
    ack_counter: SeqCounter,
    ack_tracker: SeqTracker,
    received_counter: SeqCounter,
    received_tracker: SeqTracker,
    presence_counter: SeqCounter,
    presence_tracker: SeqTracker,
    job_counter: SeqCounter,
    job_tracker: SeqTracker,
    done_counter: SeqCounter,
    done_tracker: SeqTracker,
}

impl SeqState {
    fn new() -> Self {
        SeqState {
            offer_counter: Arc::new(Mutex::new(0)),
            offer_tracker: Arc::new(Mutex::new(HashMap::new())),
            ack_counter: Arc::new(Mutex::new(0)),
            ack_tracker: Arc::new(Mutex::new(HashMap::new())),
            received_counter: Arc::new(Mutex::new(0)),
            received_tracker: Arc::new(Mutex::new(HashMap::new())),
            presence_counter: Arc::new(Mutex::new(0)),
            presence_tracker: Arc::new(Mutex::new(HashMap::new())),
            job_counter: Arc::new(Mutex::new(0)),
            job_tracker: Arc::new(Mutex::new(HashMap::new())),
            done_counter: Arc::new(Mutex::new(0)),
            done_tracker: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// 入力行の先頭にある `/qos0 ` `/qos1 ` `/qos2 ` プレフィックスを読み取り、
/// (QoS, プレフィックスを除いた本文) を返す。プレフィックスが無ければQoS1（AtLeastOnce）扱い。
fn parse_qos_prefix(line: &str) -> (QoS, &str) {
    for (prefix, qos) in [
        ("/qos0 ", QoS::AtMostOnce),  // QoS0 = 送りっぱなし（届く保証なし、その代わり速い）
        ("/qos1 ", QoS::AtLeastOnce), // QoS1 = 最低1回は届く（重複する可能性あり）
        ("/qos2 ", QoS::ExactlyOnce), // QoS2 = ちょうど1回だけ届く（一番確実、その代わり遅い）
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return (qos, rest);
        }
    }
    (QoS::AtLeastOnce, line)
}

/// 自分自身のIPアドレス（相手から接続してもらうためのアドレス）を推測する。
///
/// 仕組み: 実際には通信しないUDPソケットを作り、ブローカーのアドレスへ`connect`だけしてみる。
/// すると、OSが「そのアドレス宛てに通信するならこのネットワークインターフェースを使う」と
/// 判断してくれるので、そのローカル側アドレスを覗き見ることで自分のIPが分かる、という小技。
/// （ローカルでの動作確認用の簡易実装。インターネット越しやNAT環境では正しく動かないことがある）
fn detect_local_ip(broker_host: &str, broker_port: u16) -> String {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("UDPソケットの作成に失敗しました");
    socket
        .connect((broker_host, broker_port))
        .expect("ブローカーへの疑似接続に失敗しました");
    socket
        .local_addr()
        .expect("ローカルアドレスの取得に失敗しました")
        .ip()
        .to_string()
}

/// ファイルの中身を、繋いだ相手(TcpStream)へ送りつける。
///
/// 自前の単純な通信ルール（プロトコル）を使う:
///   1. id（OFFER/ACKと同じ申し出id）の長さ(u32)＋中身（UTF-8バイト列）
///   2. ファイル名の長さ(u32、4バイト・ビッグエンディアン)＋中身（UTF-8のバイト列）
///   3. ファイルサイズ(u64、8バイト・ビッグエンディアン)
///   4. ファイルの中身そのもの
/// idを一緒に送ることで、受け取った側が「これはどの申し出に対応する受信か」を
/// MQTTでの受信完了通知に含められるようになる。受け取る側(receive_one_file関数)は
/// この順番通りに読み取る。
fn send_file_to(host: &str, port: u16, id: &str, path: &Path) -> io::Result<()> {
    // TcpStream::connect で相手(host:port)へTCP接続する
    let mut stream = TcpStream::connect((host, port))?;

    // to_be_bytes(): 数値を「決まった順番のバイト列」に変換する（ネットワーク越しにやり取りする定番の方法）
    let id_bytes = id.as_bytes();
    stream.write_all(&(id_bytes.len() as u32).to_be_bytes())?;
    stream.write_all(id_bytes)?;

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let filename_bytes = filename.as_bytes();
    stream.write_all(&(filename_bytes.len() as u32).to_be_bytes())?;
    stream.write_all(filename_bytes)?;

    let mut file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    stream.write_all(&size.to_be_bytes())?;

    // io::copy はファイルの中身を丸ごと（読みながら少しずつ）TCPストリームへ流し込んでくれる
    io::copy(&mut file, &mut stream)?;
    Ok(())
}

/// 固定ポートのTcpListenerで、来た接続を延々と受け付け続ける（マイコン役が起動時に1回呼ぶ）。
/// 接続が来るたびに別スレッドを立てて受信処理をし、このループ自体は次の接続を待ち続ける。
/// これにより、何度でも（複数回・複数相手から）ファイルを受け取れる「常時待ち受け」になる。
///
/// client・my_name・received_topic_prefixは、受信結果（成功/失敗）をMQTTで報告するために使う。
/// バルクデータ（ファイルの中身）はMQTTの外（生TCP）でやり取りしているので、この報告が無いと
/// MQTTだけを見ている側からは「実際に届いたかどうか」が一切分からなくなってしまう。
/// 報告先トピックは`<received_topic_prefix>/<my_name>`（例: "chat/file/received/device1"）。
fn run_file_listener(
    listener: TcpListener,
    client: Client,
    my_name: String,
    received_topic_prefix: String,
    seq: SeqState,
) {
    let received_topic = format!("{received_topic_prefix}/{my_name}");
    // listener.incoming() は「接続が来るたびに1つずつ返してくれる、終わりのないイテレータ」
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[system] 接続の受け付けに失敗しました: {e}");
                continue;
            }
        };
        let client = client.clone();
        let my_name = my_name.clone();
        let received_topic = received_topic.clone();
        let seq = seq.clone();
        // 1件の受信処理に時間がかかっても次の接続を待てるように、別スレッドに任せる
        thread::spawn(move || {
            match receive_one_file(stream, &my_name, &client, &received_topic, &seq.received_counter) {
                Ok((id, filename, size, path)) => {
                    println!("[system] ジョブ{id}: 受信完了 {filename} ({size} bytes) -> {}", path.display());
                }
                Err(e) => eprintln!("[system] ファイル受信エラー: {e}"),
            }
        });
    }
}

/// すでに繋がっている1本のTCP接続(stream)から、send_file_toが送ってきたファイルを読み取って保存する。
/// 戻り値は (id, 受け取ったファイル名, サイズ, 保存先パス)。
///
/// idが読み取れた後は、途中で失敗しても成功しても、必ずreceived_topicへMQTTで結果を
/// publishする（"RECEIVED|id|自分の名前|OK or FAILED|受信できたバイト数"）。
/// これでPC側（や他の監視者）は、MQTTを見ているだけで生TCP転送の結果まで分かるようになる。
fn receive_one_file(
    mut stream: TcpStream,
    my_name: &str,
    client: &Client,
    received_topic: &str,
    seq_counter: &SeqCounter,
) -> io::Result<(String, String, u64, PathBuf)> {
    // idの長さ(4バイト)と中身を読み取る
    let mut id_len_buf = [0u8; 4];
    stream.read_exact(&mut id_len_buf)?;
    let id_len = u32::from_be_bytes(id_len_buf) as usize;
    let mut id_buf = vec![0u8; id_len];
    stream.read_exact(&mut id_buf)?;
    let id = String::from_utf8_lossy(&id_buf).to_string();

    // ここから先で失敗したら、idが分かっているのでMQTTへ"failed"を報告してからエラーを返す
    let result = receive_file_body(&mut stream);
    let report = |status: &str, size: u64| {
        let msg = ReceivedMsg {
            id: id.clone(),
            who: my_name.to_string(),
            status: status.to_string(),
            size,
            seq: next_seq(seq_counter),
        };
        // serde_json::to_vec: 構造体をJSONのバイト列に変換する
        let payload = serde_json::to_vec(&msg).unwrap();
        // publish自体が失敗しても、ここでできることは無いので無視する
        let _ = client.publish(received_topic, QoS::AtLeastOnce, false, payload);
    };

    match result {
        Ok((filename, size, save_path)) => {
            report("ok", size);
            Ok((id, filename, size, save_path))
        }
        Err(e) => {
            report("failed", 0);
            Err(e)
        }
    }
}

/// receive_one_fileのうち、「idを読んだ後」のファイル名・サイズ・中身を読み取る部分だけを
/// 切り出した関数。戻り値は (受け取ったファイル名, サイズ, 保存先パス)。
fn receive_file_body(stream: &mut TcpStream) -> io::Result<(String, u64, PathBuf)> {
    // ファイル名の長さ(4バイト)を読み取る
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let name_len = u32::from_be_bytes(len_buf) as usize;

    // ファイル名の中身を読み取る
    let mut name_buf = vec![0u8; name_len];
    stream.read_exact(&mut name_buf)?;
    let filename = String::from_utf8_lossy(&name_buf).to_string();

    // ファイルサイズ(8バイト)を読み取る
    let mut size_buf = [0u8; 8];
    stream.read_exact(&mut size_buf)?;
    let size = u64::from_be_bytes(size_buf);

    // 送られてきたファイル名そのままだと元のファイルを上書きしかねないので、頭に "received_" を付けて保存する
    let save_path = PathBuf::from(format!("received_{filename}"));
    let mut out_file = fs::File::create(&save_path)?;

    // stream.take(size): 「このストリームから最大size バイトだけ読む」という制限付きの読み取り口を作る
    // （これが無いと「ファイルの終わり」がどこか分からず、ずっと読み続けようとしてしまう）
    let mut limited = stream.take(size);
    io::copy(&mut limited, &mut out_file)?;

    Ok((filename, size, save_path))
}

/// `OfferMsg`（JSON）の送信申し出メッセージを受け取ったときの処理。
/// 自分宛て(to == my_name)のときだけ反応する。
///
/// マイコン役（起動時に固定ポートで常時listenしている＝listen_portがSome）なら、
/// 「ここに繋いで」という返事(ACK)を今のIPアドレス＋固定ポートでackトピックへpublishするだけでよい
/// （待ち受け自体はもう起動時から動いているので、ここで新しく始める必要はない）。
///
/// パソコン役（listenしていない＝listen_portがNone）は、そもそもファイルを受け取れないので、
/// 警告だけ表示してACKは返さない（＝送信側はいつまでもACKが来ないので送信を諦めることになる）。
fn handle_offer(
    payload: &str,
    my_name: &str,
    client: &Client,
    ack_topic_prefix: &str,
    broker_host: &str,
    broker_port: u16,
    listen_port: Option<u16>,
    seq: &SeqState,
) {
    // serde_json::from_str: JSON文字列を構造体に変換する。形が合わない・壊れたJSONならErrになる
    let Ok(offer) = serde_json::from_str::<OfferMsg>(payload) else {
        return;
    };
    check_seq(&offer.from, offer.seq, &seq.offer_tracker, false);

    let Some(port) = listen_port else {
        println!(
            "[system] {}さんから {} ({} bytes) を送ろうとしていますが、\
             このクライアントは受信listenをしていないので受け取れません（起動時にlisten_portを指定してください）",
            offer.from, offer.filename, offer.size
        );
        return;
    };

    // DHCPなどでIPアドレスが変わっている可能性があるので、返事のたびに毎回調べ直す（ポート番号は固定のまま）
    let host = detect_local_ip(broker_host, broker_port);
    println!(
        "[system] {}さんから {} ({} bytes) を受け取ります（{host}:{port} で待ち受け中）",
        offer.from, offer.filename, offer.size
    );

    // 返事は申し出てきた相手(offer.from)専用のトピックへ送り返す
    // （例: ack_topic_prefixが"chat/file/ack"、offer.fromが"pc1"なら"chat/file/ack/pc1"）
    let ack_topic = format!("{ack_topic_prefix}/{}", offer.from);
    let ack = AckMsg {
        id: offer.id,
        from: my_name.to_string(),
        host,
        port,
        seq: next_seq(&seq.ack_counter),
    };
    let payload = serde_json::to_vec(&ack).unwrap();
    client.publish(&ack_topic, QoS::AtLeastOnce, false, payload).unwrap();
}

/// `AckMsg`（JSON）の返事を受け取ったときの処理。
/// 自分が送った申し出(id)に対する返事だった場合、そのidに対応するファイルを
/// 教えてもらったhost:portへ実際に送信する。
fn handle_ack(payload: &str, pending_offers: &PendingOffers, seq: &SeqState) {
    let Ok(ack) = serde_json::from_str::<AckMsg>(payload) else {
        return;
    };
    check_seq(&ack.from, ack.seq, &seq.ack_tracker, false);

    // remove() は「辞書から取り出して削除する」。自分が送った申し出でなければNoneが返り何もしない
    let Some(path) = pending_offers.lock().unwrap().remove(&ack.id) else {
        return;
    };

    println!("[system] {}:{} へ接続してファイルを送信します…", ack.host, ack.port);

    // ファイル送信も時間がかかるので別スレッドに任せ、キーボード入力の受付は止めないようにする
    // 受信できたかどうかの結果はreceived_topic経由でMQTTで報告されるので、ここでは送信できたか
    // （相手に届くところまで送り切れたか）だけを表示する
    thread::spawn(move || match send_file_to(&ack.host, ack.port, &ack.id, &path) {
        Ok(()) => println!("[system] 送信完了: {}", path.display()),
        Err(e) => eprintln!("[system] ファイル送信エラー: {e}"),
    });
}

/// `ReceivedMsg`（JSON）の、生TCPでのファイル転送結果の報告を受け取ったときの処理。
/// ここではMQTT上に流れてきた結果をそのまま表示するだけだが、実運用ではここで
/// 「失敗した分だけ再送する」といった処理を足していくことになる。
fn handle_file_received(payload: &str, seq: &SeqState) {
    let Ok(received) = serde_json::from_str::<ReceivedMsg>(payload) else {
        return;
    };
    check_seq(&received.who, received.seq, &seq.received_tracker, false);

    if received.status == "ok" {
        println!(
            "[system] ジョブ{}: {} が受信完了しました（{} bytes）",
            received.id, received.who, received.size
        );
    } else {
        println!("[system] ジョブ{}: {} での受信に失敗しました", received.id, received.who);
    }
}

/// presenceトピック（`<topic>/presence/<名前>`）への`PresenceMsg`（JSON）通知を受け取ったときの処理。
/// statusが"online"ならその名前を名簿に追加、それ以外（LWTによる"offline"）なら名簿から外す。
fn handle_presence(
    publish_topic: &str,
    payload: &str,
    presence_prefix: &str,
    roster: &Roster,
    seq: &SeqState,
) {
    // 例: presence_prefixが"chat/presence"なら、"chat/presence/device1" から "device1" を取り出す
    let Some(who) = publish_topic.strip_prefix(presence_prefix).and_then(|s| s.strip_prefix('/'))
    else {
        return;
    };
    let Ok(presence) = serde_json::from_str::<PresenceMsg>(payload) else {
        return;
    };
    // "online"はSparkplug BでいうBIRTH相当（再接続でseqが0から数え直される）ので、
    // 抜け検知をリセットするためis_birth=trueで呼ぶ
    check_seq(who, presence.seq, &seq.presence_tracker, presence.status == "online");

    let mut roster = roster.lock().unwrap();
    if presence.status == "online" {
        if roster.insert(who.to_string(), true) != Some(true) {
            println!("[system] {who} がオンラインになりました");
        }
    } else if roster.remove(who).is_some() {
        println!("[system] {who} がオフラインになりました");
    }
}

/// `JobMsg`（JSON）のジョブ配信メッセージを受け取ったときの処理（マイコン役だけが反応する）。
/// 実際の機器では「内容」に応じて印刷やモーター制御などをするところだが、このサンプルでは
/// 少し待つ(sleep)ことで「処理に時間がかかる」ことだけを再現し、終わったら完了報告を返す。
fn handle_job(payload: &str, my_name: &str, client: &Client, job_done_topic_prefix: &str, seq: &SeqState) {
    let Ok(job) = serde_json::from_str::<JobMsg>(payload) else {
        return;
    };
    check_seq(&job.from, job.seq, &seq.job_tracker, false);

    println!("[system] ジョブ{}を受信: {}（処理中…）", job.id, job.content);

    let my_name = my_name.to_string();
    let client = client.clone();
    // 完了報告は自分専用のトピックへ送る（例: "chat/job/done/device1"）
    let job_done_topic = format!("{job_done_topic_prefix}/{my_name}");
    let done_counter = Arc::clone(&seq.done_counter);

    // 処理は時間がかかりうるので別スレッドに任せ、その間もMQTTの受信ループは止めない
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1)); // ここが実際の印刷・処理にあたる部分（今はダミー）
        println!("[system] ジョブ{}の処理が完了しました", job.id);
        let done = DoneMsg { id: job.id, who: my_name, seq: next_seq(&done_counter) };
        let payload = serde_json::to_vec(&done).unwrap();
        client.publish(&job_done_topic, QoS::AtLeastOnce, false, payload).unwrap();
    });
}

/// `DoneMsg`（JSON）の完了報告を受け取ったときの処理。
/// 自分が今配信中のジョブ(id)への報告であれば、/jobコマンドを実行して待っているスレッドへ、
/// チャンネル(tx)を通じて「この名前は完了した」と伝える。
fn handle_job_done(payload: &str, inflight: &InFlightState, seq: &SeqState) {
    let Ok(done) = serde_json::from_str::<DoneMsg>(payload) else {
        return;
    };
    check_seq(&done.who, done.seq, &seq.done_tracker, false);

    let guard = inflight.lock().unwrap();
    if let Some(job) = guard.as_ref() {
        if job.id == done.id {
            // send()が失敗するのは、待っている側が既にタイムアウトして諦めた後くらいなので無視してよい
            let _ = job.tx.send(done.who);
        }
    }
}

// Rustのプログラムは main関数 から実行が始まる
fn main() {
    // --- ① コマンドライン引数（起動時に渡した文字列）を読み取る ---
    let mut args = std::env::args().skip(1);

    let name = args.next().unwrap_or_else(|| {
        eprintln!("usage: mqtt-client <name> [host] [port] [topic] [listen_port]");
        std::process::exit(1);
    });
    let host = args.next().unwrap_or_else(|| "127.0.0.1".to_string());
    let port: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1883);
    let topic = args.next().unwrap_or_else(|| "chat".to_string());
    // 5番目の引数（省略可）: これを指定すると「マイコン役」になり、起動時からこのポートで
    // ファイル受信のTCP待ち受けをし続ける。省略時は「パソコン役」＝一切listenしない
    // （0を指定した場合もlistenしない扱いにする）。
    let listen_port: Option<u16> = args
        .next()
        .and_then(|s| s.parse().ok())
        .filter(|&p: &u16| p != 0);

    // ファイル送信・presence・ジョブ関連のトピックを、チャットのトピックから派生させる。
    //
    // Sparkplug B（前々回説明した産業IoT向けMQTT規約）を参考に、「宛先や報告元の名前を
    // トピックの中に組み込む」構造にしている。例えばOFFERなら"chat/file/offer/<宛先名>"、
    // presenceは元々"chat/presence/<名前>"だった。こうしておくと、
    //   - 受け取る側は「自分宛てのトピックだけ」を購読すればよく、関係ないOFFERを
    //     ペイロードの中身を見て捨てる、という無駄が無くなる
    //   - 観測する側（パソコン役やログ収集）は"chat/file/received/+"のようにワイルドカードで
    //     購読すれば、誰から来たものかトピック名だけで分かる
    // という利点がある。
    let offer_prefix = format!("{topic}/file/offer"); // 個々には "chat/file/offer/<宛先名>"
    let my_offer_topic = format!("{offer_prefix}/{name}");
    let ack_prefix = format!("{topic}/file/ack"); // 個々には "chat/file/ack/<申し出た人の名前>"
    let my_ack_topic = format!("{ack_prefix}/{name}");
    // 生TCPでのファイル転送が終わった後、マイコン側が結果(成功/失敗)を報告するためのトピック接頭辞。
    // これがMQTT上にあることで、MQTTだけを見ている人にも転送結果が分かるようになる
    let received_prefix = format!("{topic}/file/received"); // 個々には "chat/file/received/<マイコン名>"
    // マイコンの在室確認(presence)用のトピック接頭辞
    let presence_prefix = format!("{topic}/presence"); // 個々のpresenceトピックは "chat/presence/<名前>" になる
    let my_presence_topic = format!("{presence_prefix}/{name}");
    // ジョブは全マイコンへの一斉配信なので宛先を絞らずブローカーへそのままpublishする
    let job_topic = format!("{topic}/job/queue");
    // ジョブの完了報告は、報告してきたマイコンの名前をトピックに含める
    let job_done_prefix = format!("{topic}/job/done"); // 個々には "chat/job/done/<マイコン名>"

    // メッセージ種類ごとのseqカウンタ・トラッカーをまとめて用意する
    let seq = SeqState::new();

    // --- ② MQTT接続の設定を作る ---
    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    // マイコン役だけ、自分のpresenceトピックにLast Will（異常切断時に代わりにブローカーが
    // publishしてくれる遺言メッセージ）を登録しておく。こうしておくと、電源断や通信断など
    // 「さようなら」を言えずに落ちた場合でも、ブローカーが自動で"offline"を配ってくれる。
    // retain=trueにしているので、後からpresence topicをsubscribeしたクライアントにも
    // 「今の状態」がすぐ届く。
    if listen_port.is_some() {
        // 遺言メッセージのseqは0固定でよい（実際のカウンタはまだ動き出していない、DEATHは
        // 「一番最後」の1通なので後続との抜け比較は発生しない）
        let offline = serde_json::to_vec(&PresenceMsg { status: "offline".to_string(), seq: 0 }).unwrap();
        mqttoptions.set_last_will(LastWill::new(&my_presence_topic, offline, QoS::AtLeastOnce, true));
    }

    // --- ③ 実際にMQTTブローカー（サーバー）へ接続する ---
    let (client, mut connection) = Client::new(mqttoptions, 10);

    // チャット用トピックに加えて、ファイル送信の申し出・返事、presence(在室確認)、
    // ジョブ配信・完了報告のトピックも購読する。
    // 自分宛てのOFFER/ACKは自分の名前が付いたトピックだけを購読すればよい。
    // "+" は1階層ぶんのワイルドカードなので、"chat/file/received/+" で全マイコン分の結果を
    // まとめて拾える（presenceも元々同じ考え方で"chat/presence/+"にしている）。
    client.subscribe(&topic, QoS::AtMostOnce).unwrap();
    client.subscribe(&my_offer_topic, QoS::AtLeastOnce).unwrap();
    client.subscribe(&my_ack_topic, QoS::AtLeastOnce).unwrap();
    client
        .subscribe(format!("{received_prefix}/+"), QoS::AtLeastOnce)
        .unwrap();
    client
        .subscribe(format!("{presence_prefix}/+"), QoS::AtLeastOnce)
        .unwrap();
    client.subscribe(&job_topic, QoS::AtLeastOnce).unwrap();
    client
        .subscribe(format!("{job_done_prefix}/+"), QoS::AtLeastOnce)
        .unwrap();

    // マイコン役は、接続できたらすぐ自分のpresenceトピックに"online"をretain付きでpublishする。
    // retain=trueにしておくことで、後からブローカーに繋いだ（=あとから/jobを実行する）パソコン役にも、
    // 「このマイコンは今オンラインだ」という状態がすぐに伝わる。
    if listen_port.is_some() {
        let online = serde_json::to_vec(&PresenceMsg {
            status: "online".to_string(),
            seq: next_seq(&seq.presence_counter),
        })
        .unwrap();
        client.publish(&my_presence_topic, QoS::AtLeastOnce, true, online).unwrap();
    }

    // 自分が送信を申し出た(まだ返事を待っている)ファイルの一覧
    let pending_offers: PendingOffers = Arc::new(Mutex::new(HashMap::new()));
    // 現在オンラインになっているマイコンの名簿（presence通知で更新され続ける）
    let roster: Roster = Arc::new(Mutex::new(HashMap::new()));
    // 今配信中で完了報告を待っているジョブ（無ければNone）
    let inflight: InFlightState = Arc::new(Mutex::new(None));

    // listen_portが指定されている（＝マイコン役）なら、起動時に一度だけ固定ポートでlistenを開始し、
    // そのままプログラムが終わるまでファイル受信を待ち受け続ける
    if let Some(port) = listen_port {
        let listener = TcpListener::bind(("0.0.0.0", port))
            .unwrap_or_else(|e| panic!("{port}番ポートでのlisten開始に失敗しました: {e}"));
        println!("[system] {port}番ポートで常時待ち受けを開始しました（マイコン役）");
        let client = client.clone();
        let name = name.clone();
        let received_prefix = received_prefix.clone();
        let seq = seq.clone();
        thread::spawn(move || run_file_listener(listener, client, name, received_prefix, seq));
    }

    // --- ④ 別スレッドを立てて「キーボード入力 → メッセージ送信」を担当させる ---
    {
        let client = client.clone();
        let name = name.clone();
        let topic = topic.clone();
        let offer_prefix = offer_prefix.clone();
        let pending_offers = Arc::clone(&pending_offers);
        let job_topic = job_topic.clone();
        let roster = Arc::clone(&roster);
        let inflight = Arc::clone(&inflight);
        let seq = seq.clone();

        thread::spawn(move || {
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                if line.is_empty() {
                    continue;
                }

                // "/job 内容" と入力されたら、今オンラインの全マイコンへ一斉配信し、
                // 全員から完了報告が来る（かタイムアウトする）までここで待ってから次の行を受け付ける。
                // 「1件投げたら、それが終わってから次を投げる」という直列実行を実現するための処理。
                if let Some(content) = line.strip_prefix("/job ") {
                    // ロックした瞬間の名簿を「今回のジョブの宛先」として写し取る（HashSetにcloneする）
                    let targets: HashSet<String> = roster
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|(_, &online)| online)
                        .map(|(who, _)| who.clone())
                        .collect();

                    if targets.is_empty() {
                        println!("[system] 今オンラインのマイコンがいないため、ジョブを送信できません");
                        continue;
                    }

                    let nanos = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos();
                    let id = format!("{name}-{nanos}");

                    // このジョブの完了報告を受け取るための通信路(チャンネル)を作り、
                    // メインスレッド(handle_job_done)が使えるよう共有状態にセットしておく
                    let (tx, rx) = mpsc::channel::<String>();
                    *inflight.lock().unwrap() = Some(InFlightJob { id: id.clone(), tx });

                    let job = JobMsg {
                        id: id.clone(),
                        from: name.clone(),
                        content: content.to_string(),
                        seq: next_seq(&seq.job_counter),
                    };
                    let payload = serde_json::to_vec(&job).unwrap();
                    client.publish(&job_topic, QoS::AtLeastOnce, false, payload).unwrap();
                    println!(
                        "[system] ジョブ{id}を{}台のマイコン({targets:?})へ配信しました。完了を待っています…",
                        targets.len()
                    );

                    // 全員分の完了報告が来るか、タイムアウトするまでここでブロックして待つ
                    let mut remaining = targets;
                    let deadline = Instant::now() + JOB_TIMEOUT;
                    while !remaining.is_empty() {
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        // recv_timeout: 「残り時間内に何か届けばそれを返す、届かなければタイムアウトを返す」
                        match rx.recv_timeout(deadline - now) {
                            Ok(who) => {
                                remaining.remove(&who);
                            }
                            Err(_) => break, // タイムアウト（これ以上待っても来ない）
                        }
                    }
                    *inflight.lock().unwrap() = None; // 待つのをやめたので、共有状態も片付ける

                    if remaining.is_empty() {
                        println!("[system] ジョブ{id}は全員完了しました");
                    } else {
                        println!("[system] エラー: ジョブ{id}は次のマイコンから応答がありませんでした: {remaining:?}");
                    }
                    continue;
                }

                // "/send 宛先の名前 ファイルパス" と入力されたら、チャットではなくファイル送信の申し出として扱う
                if let Some(rest) = line.strip_prefix("/send ") {
                    // split_once(' ') で「最初のスペースの前後」に文字列を2つに割る
                    let Some((to, path_str)) = rest.split_once(' ') else {
                        println!("[system] 使い方: /send <宛先の名前> <ファイルパス>");
                        continue;
                    };
                    let path = PathBuf::from(path_str);
                    let metadata = match fs::metadata(&path) {
                        Ok(m) => m,
                        Err(e) => {
                            println!("[system] ファイルが読めません: {path_str} ({e})");
                            continue;
                        }
                    };
                    let filename = path
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| path_str.to_string());

                    // 今の時刻(ナノ秒)を使って、他の申し出とかぶらないID文字列を作る
                    let nanos = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos();
                    let id = format!("{name}-{nanos}");

                    // 「id → 送るファイルのパス」を覚えておく（相手からACKが来たときに使う）
                    pending_offers.lock().unwrap().insert(id.clone(), path);

                    let offer = OfferMsg {
                        id,
                        from: name.clone(),
                        to: to.to_string(),
                        filename: filename.clone(),
                        size: metadata.len(),
                        seq: next_seq(&seq.offer_counter),
                    };
                    // 宛先の名前を組み込んだトピック（例: "chat/file/offer/device1"）へpublishする
                    let offer_topic = format!("{offer_prefix}/{to}");
                    let payload = serde_json::to_vec(&offer).unwrap();
                    client.publish(&offer_topic, QoS::AtLeastOnce, false, payload).unwrap();
                    println!(
                        "[system] {to}へ {filename} ({} bytes) の送信を申し出ました。相手の応答を待っています…",
                        metadata.len()
                    );
                    continue;
                }

                // ここに来たら普通のチャットメッセージ
                let (qos, text) = parse_qos_prefix(&line);
                let message = format!("{name}: {text}");
                client
                    .publish(&topic, qos, false, message.as_bytes())
                    .unwrap();
            }
        });
    }

    println!("接続しました host={host} port={port} topic={topic} name={name}");
    println!("メッセージを入力して Enter で送信します（Ctrl+D で終了）");
    println!("先頭に /qos0 /qos1 /qos2 を付けるとそのメッセージだけQoSを変更できます（省略時はQoS1）");
    println!("/send <宛先の名前> <ファイルパス> でファイルを送れます（例: /send bob ./photo.png）");
    println!("/job <内容> で、今オンラインの全マイコンへ一斉配信し、全員完了するまで待ちます（例: /job print A4x3）");
    match listen_port {
        Some(p) => println!("役割: マイコン役（{p}番ポートで常時待ち受け中。他のクライアントからファイルを受け取れます）"),
        None => println!("役割: パソコン役（listenはしません。ファイルは送るだけで、受け取ることはできません）"),
    }

    // --- ⑤ メインスレッドでは「受信したメッセージを表示する」処理をずっと続ける ---
    for notification in connection.iter() {
        match notification {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                let text = String::from_utf8_lossy(&publish.payload);

                if publish.topic == my_offer_topic {
                    handle_offer(&text, &name, &client, &ack_prefix, &host, port, listen_port, &seq);
                } else if publish.topic == my_ack_topic {
                    handle_ack(&text, &pending_offers, &seq);
                } else if publish.topic.starts_with(&received_prefix) {
                    handle_file_received(&text, &seq);
                } else if publish.topic.starts_with(&presence_prefix) {
                    handle_presence(&publish.topic, &text, &presence_prefix, &roster, &seq);
                } else if publish.topic == job_topic {
                    // ジョブに反応して実際に処理をするのはマイコン役（listen_portを指定している側）だけ。
                    // パソコン役はジョブを配る側なので、自分宛ての配信を受け取っても無視する。
                    if listen_port.is_some() {
                        handle_job(&text, &name, &client, &job_done_prefix, &seq);
                    }
                } else if publish.topic.starts_with(&job_done_prefix) {
                    handle_job_done(&text, &inflight, &seq);
                } else if publish.topic == topic {
                    println!("{text}");
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("接続エラー: {e:?}");
                break;
            }
        }
    }
}
