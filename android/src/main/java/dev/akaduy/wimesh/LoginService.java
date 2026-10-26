package dev.akaduy.wimesh;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.net.NetworkRequest;
import android.os.IBinder;
import android.os.PowerManager;

import java.util.concurrent.Semaphore;
import java.util.concurrent.TimeUnit;

/**
 * The Android `--auto`: a foreground service that runs `--auto --once` every 30 seconds
 * and right away when Wi-Fi connects. On Wi-MESH it keeps the CPU awake so the screen
 * turning off does not pause the checks.
 */
public class LoginService extends Service {
    private static final String CHANNEL = "auto";
    private static final long INTERVAL_SECONDS = 30;

    private final Semaphore wake = new Semaphore(0);
    private volatile boolean running;
    private PowerManager.WakeLock wakeLock;
    private ConnectivityManager.NetworkCallback callback;

    static void start(Context context) {
        context.startForegroundService(new Intent(context, LoginService.class));
    }

    static void stop(Context context) {
        context.stopService(new Intent(context, LoginService.class));
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        getSystemService(NotificationManager.class).createNotificationChannel(
                new NotificationChannel(CHANNEL, "Auto login", NotificationManager.IMPORTANCE_MIN));
        startForeground(1, new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_notify_sync)
                .setContentTitle("Keeping Wi-MESH logged in")
                .build());
        if (!LoginJob.hasCredentials(this)) {
            stopSelf();
            return START_NOT_STICKY;
        }
        if (!running) {
            running = true;
            wakeLock = getSystemService(PowerManager.class)
                    .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "wimesh:auto");
            wakeLock.setReferenceCounted(false);
            new Thread(this::loop).start();
            callback = new ConnectivityManager.NetworkCallback() {
                @Override
                public void onAvailable(Network network) {
                    wake.release();
                }
            };
            getSystemService(ConnectivityManager.class).registerNetworkCallback(
                    new NetworkRequest.Builder()
                            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
                            .build(),
                    callback);
        }
        return START_STICKY;
    }

    private void loop() {
        while (running) {
            LoginJob.run(this);
            // Off Wi-MESH the phone may sleep; connecting to Wi-Fi wakes the loop again
            if (LoginJob.onWiMesh(this)) {
                wakeLock.acquire();
            } else {
                wakeLock.release();
            }
            try {
                wake.tryAcquire(INTERVAL_SECONDS, TimeUnit.SECONDS);
            } catch (InterruptedException ignored) {
            }
            wake.drainPermits();
        }
        wakeLock.release();
    }

    @Override
    public void onDestroy() {
        running = false;
        wake.release();
        if (callback != null) {
            getSystemService(ConnectivityManager.class).unregisterNetworkCallback(callback);
        }
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
