package dev.akaduy.wimesh;

import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.net.ConnectivityManager;
import android.net.NetworkCapabilities;
import android.net.NetworkRequest;
import android.os.Build;

/**
 * Wakes the app only when Wi-Fi connects, so nothing runs or polls in between.
 * The system keeps the PendingIntent callback until reboot, so boot re-registers it.
 */
public class NetworkReceiver extends BroadcastReceiver {
    private static final String ACTION_WIFI = "dev.akaduy.wimesh.WIFI";

    static void register(Context context) {
        int flags = PendingIntent.FLAG_UPDATE_CURRENT;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            // The system adds the network to the intent
            flags |= PendingIntent.FLAG_MUTABLE;
        }
        PendingIntent intent = PendingIntent.getBroadcast(context, 0,
                new Intent(context, NetworkReceiver.class).setAction(ACTION_WIFI), flags);
        NetworkRequest request = new NetworkRequest.Builder()
                .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
                .build();
        context.getSystemService(ConnectivityManager.class).registerNetworkCallback(request, intent);
    }

    @Override
    public void onReceive(Context context, Intent intent) {
        if (ACTION_WIFI.equals(intent.getAction())) {
            if (LoginJob.hasCredentials(context)) {
                LoginJob.schedule(context);
            }
        } else if (LoginJob.hasCredentials(context)) {
            // BOOT_COMPLETED or MY_PACKAGE_REPLACED
            register(context);
        }
    }
}
