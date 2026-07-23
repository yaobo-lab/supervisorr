这个项目的守护原理，本质上是：**守护进程作为父进程启动目标程序，然后等待目标子进程退出。**

它不是定时扫描系统进程，也不是靠 PID 文件猜测程序是否存在。

## 它怎么知道要守护哪些程序

启动 Daemon 时读取 TOML 配置：

```toml
[program.my_app]
command = "node index.js"
autostart = true
autorestart = true
```

配置里的每个 `[program.xxx]` 都是一个需要管理的程序。

Daemon 启动后遍历这些配置，为每个程序创建一个独立的 Tokio 监督任务：

```rust
for (name, prog_config) in config.program {
    tokio::spawn(async move {
        supervise_program(name, prog_config, state).await;
    });
}
```

逻辑上类似：

```text
supervisorr
├─ 监督任务：my_app
├─ 监督任务：metasearch
└─ 监督任务：worker
```

这些是异步任务，不是额外的操作系统线程或守护进程。

## 它怎么启动目标程序

每个监督任务进入一个无限循环，然后执行：

```rust
let mut cmd = Command::new("sh");
cmd.arg("-c").arg(&config.command);

let mut child = cmd.spawn()?;
```

例如配置：

```toml
command = "node index.js"
```

实际执行的是：

```bash
sh -c "node index.js"
```

`spawn()` 成功后会得到一个 `Child` 对象。这个对象代表刚刚创建的子进程，可以取得它的 PID：

```rust
let pid = child.id();
```

然后项目把状态更新为：

```text
Running(pid)
```

## 它怎么知道程序退出了

关键是：

```rust
let status = child.wait().await;
```

`wait()` 的意思是等待这个子进程结束。

这里是异步等待，所以不会把整个守护进程卡住。目标程序运行期间，操作系统保留其进程状态；程序退出时，操作系统通知父进程，Tokio 唤醒对应的监督任务。

流程大致是：

```text
启动子进程
    │
    ▼
获得 Child 和 PID
    │
    ▼
child.wait().await
    │
    ├─ 程序仍在运行：监督任务休眠等待
    │
    └─ 程序退出：操作系统唤醒监督任务
                │
                ▼
          得到退出状态
```

退出状态可能包括：

```text
正常退出     → exit code 0
异常退出     → exit code 1、2……
被信号终止   → Unix 下可能没有普通退出码
等待失败     → Rust 返回错误
```

当前代码将结果保存为：

```rust
Status::Exited(exit_code)
```

## 为什么可以自动重启

`child.wait().await` 返回后，代码会继续执行监督循环。

简化后的逻辑是：

```rust
loop {
    启动程序;

    等待程序退出;

    记录退出状态;

    if 用户要求运行 && autorestart {
        等待 500ms;
        重新进入循环;
    }
}
```

所以自动重启不是由系统自动完成，而是监督任务发现 `wait()` 返回后，再调用一次 `spawn()`。

例如：

```text
12:00:00  启动 my_app，PID 1001
12:10:00  PID 1001 异常退出
12:10:00  child.wait() 返回
12:10:00  状态变为 Exited(1)
12:10:01  监督循环重新启动
12:10:01  新进程 PID 1058
```

## `start` 和 `stop` 是怎么工作的

每个进程有两个概念：

```text
intent：用户希望它处于什么状态
status：它现在实际上是什么状态
```

例如：

```text
intent = Run
status = Running(1234)
```

表示用户希望运行，而且现在确实正在运行。

### Start

执行：

```bash
supervisorr start my_app
```

CLI 通过 Unix Socket 告诉 Daemon：

```text
把 my_app 的 intent 改为 Run
```

监督任务每隔约 500ms 检查 intent，发现变成 `Run` 后启动进程。

### Stop

执行：

```bash
supervisorr stop my_app
```

Daemon 会：

1. 把 intent 改为 `Stop`；
2. 如果状态是 `Running(pid)`，向该 PID 发送 `SIGTERM`；
3. `child.wait().await` 检测到进程退出；
4. 监督循环看到 intent 是 `Stop`，不再启动。

这样可以区分两种退出：

```text
意外退出：
intent = Run
程序退出
→ 应当重启

用户主动停止：
intent = Stop
程序退出
→ 不应重启
```

## 为什么不需要不断检查 PID

一种比较初级的守护方式是定期执行：

```text
进程 PID 还存在吗？
```

但这个项目不需要这么做，因为目标进程是它自己启动的子进程，它持有 `Child` 对象，可以直接等待退出。

这比定时检查 PID更可靠：

- 不会把复用后的 PID 误认为原程序；
- 可以直接获得退出码；
- 没有定时轮询延迟；
- 不必频繁查询系统进程表；
- 父进程可以正确回收子进程，避免僵尸进程。

项目中的 500ms 轮询主要用于检查用户的 `intent`，不是用来判断子进程是否退出。

## 当前实现的一个重要局限

项目实际启动的是：

```text
supervisorr
  └─ sh -c "node index.js"
       └─ node index.js
```

`Child` 和 PID 可能对应 `sh`，不一定是最终的 `node`。

通常 shell 会等待前台命令，因此：

```rust
child.wait().await
```

依然能间接知道命令结束。但如果命令：

- 自己进入后台；
- fork 后让父进程退出；
- 创建独立子进程；
- shell 提前退出；
- 启动了多个子进程；

那么 shell 的生命周期就不一定等于真实业务程序的生命周期。

例如：

```toml
command = "node index.js &"
```

流程可能变成：

```text
sh 启动 node
sh 立即退出
child.wait() 立即返回
supervisorr 误以为程序退出
再次启动
最终产生多个 node 进程
```

这也是为什么成熟的进程管理器通常要求程序保持前台运行，并通过 Unix 进程组或 Windows Job Object 管理完整的进程树。

一句话概括：**配置告诉 supervisorr 要管理谁，`spawn()` 建立父子关系，`child.wait().await` 由操作系统通知它子进程何时退出，监督循环再根据用户意图和 `autorestart` 决定是否重启。**