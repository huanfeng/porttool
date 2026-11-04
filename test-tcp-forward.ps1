# 测试 TCP 转发功能

Write-Host "启动 echo 服务器（端口 8081）..." -ForegroundColor Green
$echoServer = Start-Job -ScriptBlock {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 8081)
    $listener.Start()
    Write-Host "Echo 服务器监听 127.0.0.1:8081"

    while ($true) {
        $client = $listener.AcceptTcpClient()
        $stream = $client.GetStream()
        $reader = [System.IO.StreamReader]::new($stream)
        $writer = [System.IO.StreamWriter]::new($stream)

        $line = $reader.ReadLine()
        Write-Host "收到: $line"
        $writer.WriteLine("ECHO: $line")
        $writer.Flush()

        $client.Close()
    }
}

Start-Sleep -Seconds 1

Write-Host "`n启动 PortTool 服务..." -ForegroundColor Green
$porttool = Start-Process -FilePath "cargo" -ArgumentList "run", "-p", "porttool", "--", "serve", "--config", "test-config.toml" -PassThru -NoNewWindow

Start-Sleep -Seconds 2

Write-Host "`n测试 TCP 转发（127.0.0.1:8080 -> 127.0.0.1:8081）..." -ForegroundColor Green
try {
    $client = [System.Net.Sockets.TcpClient]::new("127.0.0.1", 8080)
    $stream = $client.GetStream()
    $writer = [System.IO.StreamWriter]::new($stream)
    $reader = [System.IO.StreamReader]::new($stream)

    $testMsg = "Hello, TCP Forwarder!"
    Write-Host "发送: $testMsg"
    $writer.WriteLine($testMsg)
    $writer.Flush()

    $response = $reader.ReadLine()
    Write-Host "接收: $response" -ForegroundColor Cyan

    if ($response -eq "ECHO: $testMsg") {
        Write-Host "`n✓ TCP 转发测试成功！" -ForegroundColor Green
    } else {
        Write-Host "`n✗ TCP 转发测试失败" -ForegroundColor Red
    }

    $client.Close()
} catch {
    Write-Host "`n✗ 连接失败: $_" -ForegroundColor Red
}

Write-Host "`n清理资源..." -ForegroundColor Yellow
Stop-Process -Id $porttool.Id -Force
Stop-Job -Job $echoServer
Remove-Job -Job $echoServer

Write-Host "测试完成" -ForegroundColor Green
