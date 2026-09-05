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
///   1. ファイル名の長さ(u32、4バイト・ビッグエンディアン)
///   2. ファイル名の中身（UTF-8のバイト列）
///   3. ファイルサイズ(u64、8バイト・ビッグエンディアン)
///   4. ファイルの中身そのもの
/// 受け取る側(receive_file関数)はこの順番通りに読み取る。
fn send_file_to(host: &str, port: u16, path: &Path) -> io::Result<()> {
    // TcpStream::connect で相手(host:port)へTCP接続する
    let mut stream = TcpStream::connect((host, port))?;

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let filename_bytes = filename.as_bytes();

    // to_be_bytes(): 数値を「決まった順番のバイト列」に変換する（ネットワーク越しにやり取りする定番の方法）
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
fn run_file_listener(listener: TcpListener) {
    // listener.incoming() は「接続が来るたびに1つずつ返してくれる、終わりのないイテレータ」
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[system] 接続の受け付けに失敗しました: {e}");
                continue;
            }
        };
        // 1件の受信処理に時間がかかっても次の接続を待てるように、別スレッドに任せる
        thread::spawn(move || match receive_one_file(stream) {
            Ok((filename, size, path)) => {
                println!("[system] 受信完了: {filename} ({size} bytes) -> {}", path.display());
            }
            Err(e) => eprintln!("[system] ファイル受信エラー: {e}"),
        });
    }
}

/// すでに繋がっている1本のTCP接続(stream)から、send_file_toが送ってきたファイルを読み取って保存する。
/// 戻り値は (受け取ったファイル名, サイズ, 保存先パス)。
fn receive_one_file(mut stream: TcpStream) -> io::Result<(String, u64, PathBuf)> {
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
    let mut limited = (&mut stream).take(size);
    io::copy(&mut limited, &mut out_file)?;

    Ok((filename, size, save_path))
}

/// 「OFFER|id|from|to|filename|size」という送信申し出メッセージを受け取ったときの処理。
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
    ack_topic: &str,
    broker_host: &str,
    broker_port: u16,
    listen_port: Option<u16>,
) {
    // "|" で区切って各項目に分解する。件数が合わなければ壊れたメッセージとして無視する
    let parts: Vec<&str> = payload.split('|').collect();
    let [_, id, from, to, filename, size] = parts.as_slice() else {
        return;
    };
    if *to != my_name {
        return; // 自分宛てのオファーでなければ何もしない
    }

    let Some(port) = listen_port else {
        println!(
            "[system] {from}さんから {filename} ({size} bytes) を送ろうとしていますが、\
             このクライアントは受信listenをしていないので受け取れません（起動時にlisten_portを指定してください）"
        );
        return;
    };

    // DHCPなどでIPアドレスが変わっている可能性があるので、返事のたびに毎回調べ直す（ポート番号は固定のまま）
    let host = detect_local_ip(broker_host, broker_port);
    println!("[system] {from}さんから {filename} ({size} bytes) を受け取ります（{host}:{port} で待ち受け中）");

    // ここに繋いでほしい、という返事をMQTTで送り返す
    let ack = format!("ACK|{id}|{host}|{port}");
    client
        .publish(ack_topic, QoS::AtLeastOnce, false, ack.as_bytes())
        .unwrap();
}

/// 「ACK|id|host|port」という返事を受け取ったときの処理。
/// 自分が送った申し出(id)に対する返事だった場合、そのidに対応するファイルを
/// 教えてもらったhost:portへ実際に送信する。
fn handle_ack(payload: &str, pending_offers: &PendingOffers) {
    let parts: Vec<&str> = payload.split('|').collect();
    let [_, id, host, port_str] = parts.as_slice() else {
        return;
    };
    let Ok(port) = port_str.parse::<u16>() else {
        return;
    };

    // remove() は「辞書から取り出して削除する」。自分が送った申し出でなければNoneが返り何もしない
    let path = pending_offers.lock().unwrap().remove(*id);
    let Some(path) = path else {
        return;
    };

    let host = host.to_string();
    println!("[system] {host}:{port} へ接続してファイルを送信します…");

    // ファイル送信も時間がかかるので別スレッドに任せ、キーボード入力の受付は止めないようにする
    thread::spawn(move || match send_file_to(&host, port, &path) {
        Ok(()) => println!("[system] 送信完了: {}", path.display()),
        Err(e) => eprintln!("[system] ファイル送信エラー: {e}"),
    });
}

/// presenceトピック（`<topic>/presence/<名前>`）への通知を受け取ったときの処理。
/// ペイロードが"online"ならその名前を名簿に追加、それ以外（LWTによる"offline"）なら名簿から外す。
fn handle_presence(publish_topic: &str, payload: &str, presence_prefix: &str, roster: &Roster) {
    // 例: presence_prefixが"chat/presence"なら、"chat/presence/device1" から "device1" を取り出す
    let Some(who) = publish_topic.strip_prefix(presence_prefix).and_then(|s| s.strip_prefix('/'))
    else {
        return;
    };

    let mut roster = roster.lock().unwrap();
    if payload == "online" {
        if roster.insert(who.to_string(), true) != Some(true) {
            println!("[system] {who} がオンラインになりました");
        }
    } else if roster.remove(who).is_some() {
        println!("[system] {who} がオフラインになりました");
    }
}

/// 「JOB|id|内容」というジョブ配信メッセージを受け取ったときの処理（マイコン役だけが反応する）。
/// 実際の機器では「内容」に応じて印刷やモーター制御などをするところだが、このサンプルでは
/// 少し待つ(sleep)ことで「処理に時間がかかる」ことだけを再現し、終わったら完了報告を返す。
fn handle_job(payload: &str, my_name: &str, client: &Client, job_done_topic: &str) {
    let Some((id, content)) = payload.strip_prefix("JOB|").and_then(|s| s.split_once('|')) else {
        return;
    };

    println!("[system] ジョブ{id}を受信: {content}（処理中…）");

    let id = id.to_string();
    let my_name = my_name.to_string();
    let client = client.clone();
    let job_done_topic = job_done_topic.to_string();

    // 処理は時間がかかりうるので別スレッドに任せ、その間もMQTTの受信ループは止めない
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1)); // ここが実際の印刷・処理にあたる部分（今はダミー）
        println!("[system] ジョブ{id}の処理が完了しました");
        let done = format!("DONE|{id}|{my_name}");
        client
            .publish(&job_done_topic, QoS::AtLeastOnce, false, done.as_bytes())
            .unwrap();
    });
}

/// 「DONE|id|名前」という完了報告を受け取ったときの処理。
/// 自分が今配信中のジョブ(id)への報告であれば、/jobコマンドを実行して待っているスレッドへ、
/// チャンネル(tx)を通じて「この名前は完了した」と伝える。
fn handle_job_done(payload: &str, inflight: &InFlightState) {
    let Some((id, who)) = payload.strip_prefix("DONE|").and_then(|s| s.split_once('|')) else {
        return;
    };

    let guard = inflight.lock().unwrap();
    if let Some(job) = guard.as_ref() {
        if job.id == id {
            // send()が失敗するのは、待っている側が既にタイムアウトして諦めた後くらいなので無視してよい
            let _ = job.tx.send(who.to_string());
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

    // ファイル送信の申し出(offer)と、その返事(ack)専用のトピックを、チャットのトピックから派生させる
    // 例: topicが"chat"なら "chat/file/offer" と "chat/file/ack"
    let offer_topic = format!("{topic}/file/offer");
    let ack_topic = format!("{topic}/file/ack");
    // マイコンの在室確認(presence)、ジョブ配信、ジョブ完了報告用のトピック
    let presence_prefix = format!("{topic}/presence"); // 個々のpresenceトピックは "chat/presence/<名前>" になる
    let my_presence_topic = format!("{presence_prefix}/{name}");
    let job_topic = format!("{topic}/job/queue");
    let job_done_topic = format!("{topic}/job/done");

    // --- ② MQTT接続の設定を作る ---
    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    // マイコン役だけ、自分のpresenceトピックにLast Will（異常切断時に代わりにブローカーが
    // publishしてくれる遺言メッセージ）を登録しておく。こうしておくと、電源断や通信断など
    // 「さようなら」を言えずに落ちた場合でも、ブローカーが自動で"offline"を配ってくれる。
    // retain=trueにしているので、後からpresence topicをsubscribeしたクライアントにも
    // 「今の状態」がすぐ届く。
    if listen_port.is_some() {
        mqttoptions.set_last_will(LastWill::new(
            &my_presence_topic,
            "offline",
            QoS::AtLeastOnce,
            true,
        ));
    }

    // --- ③ 実際にMQTTブローカー（サーバー）へ接続する ---
    let (client, mut connection) = Client::new(mqttoptions, 10);

    // チャット用トピックに加えて、ファイル送信の申し出・返事、presence(在室確認)、
    // ジョブ配信・完了報告のトピックも購読する
    // "+" は1階層ぶんのワイルドカードなので、"chat/presence/+" で全マイコン分のpresenceをまとめて拾える
    client.subscribe(&topic, QoS::AtMostOnce).unwrap();
    client.subscribe(&offer_topic, QoS::AtLeastOnce).unwrap();
    client.subscribe(&ack_topic, QoS::AtLeastOnce).unwrap();
    client
        .subscribe(format!("{presence_prefix}/+"), QoS::AtLeastOnce)
        .unwrap();
    client.subscribe(&job_topic, QoS::AtLeastOnce).unwrap();
    client.subscribe(&job_done_topic, QoS::AtLeastOnce).unwrap();

    // マイコン役は、接続できたらすぐ自分のpresenceトピックに"online"をretain付きでpublishする。
    // retain=trueにしておくことで、後からブローカーに繋いだ（=あとから/jobを実行する）パソコン役にも、
    // 「このマイコンは今オンラインだ」という状態がすぐに伝わる。
    if listen_port.is_some() {
        client
            .publish(&my_presence_topic, QoS::AtLeastOnce, true, "online")
            .unwrap();
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
        thread::spawn(move || run_file_listener(listener));
    }

    // --- ④ 別スレッドを立てて「キーボード入力 → メッセージ送信」を担当させる ---
    {
        let client = client.clone();
        let name = name.clone();
        let topic = topic.clone();
        let offer_topic = offer_topic.clone();
        let pending_offers = Arc::clone(&pending_offers);
        let job_topic = job_topic.clone();
        let roster = Arc::clone(&roster);
        let inflight = Arc::clone(&inflight);

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

                    let job = format!("JOB|{id}|{content}");
                    client
                        .publish(&job_topic, QoS::AtLeastOnce, false, job.as_bytes())
                        .unwrap();
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

                    let offer = format!("OFFER|{id}|{name}|{to}|{filename}|{}", metadata.len());
                    client
                        .publish(&offer_topic, QoS::AtLeastOnce, false, offer.as_bytes())
                        .unwrap();
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

                if publish.topic == offer_topic {
                    handle_offer(&text, &name, &client, &ack_topic, &host, port, listen_port);
                } else if publish.topic == ack_topic {
                    handle_ack(&text, &pending_offers);
                } else if publish.topic.starts_with(&presence_prefix) {
                    handle_presence(&publish.topic, &text, &presence_prefix, &roster);
                } else if publish.topic == job_topic {
                    // ジョブに反応して実際に処理をするのはマイコン役（listen_portを指定している側）だけ。
                    // パソコン役はジョブを配る側なので、自分宛ての配信を受け取っても無視する。
                    if listen_port.is_some() {
                        handle_job(&text, &name, &client, &job_done_topic);
                    }
                } else if publish.topic == job_done_topic {
                    handle_job_done(&text, &inflight);
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
