package dev.akaduy.wimesh;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/** Restarts the login service after a reboot or an app update. */
public class BootReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        if (LoginJob.hasCredentials(context)) {
            LoginService.start(context);
        }
    }
}
