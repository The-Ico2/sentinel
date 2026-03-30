// OpenDesktop — Page Script
// Clock, toast notifications

// ── Live Clock ──
var clockH = document.getElementById('clock-h');
var clockM = document.getElementById('clock-m');
var clockS = document.getElementById('clock-s');
if (clockH && clockM && clockS) {
  function updateClock() {
    var now = new Date();
    clockH.textContent = String(now.getHours()).padStart(2, '0');
    clockM.textContent = String(now.getMinutes()).padStart(2, '0');
    clockS.textContent = String(now.getSeconds()).padStart(2, '0');
  }
  updateClock();
  setInterval(updateClock, 1000);
}

// ── Toast Notifications ──
function showToast(message, type) {
  type = type || 'info';
  var container = document.getElementById('toast-container');
  if (!container) return;
  var toast = document.createElement('div');
  toast.className = 'toast toast-' + type;
  toast.textContent = message;
  container.appendChild(toast);
  setTimeout(function() {
    toast.classList.add('toast-exit');
    setTimeout(function() {
      if (toast.parentNode) toast.parentNode.removeChild(toast);
    }, 300);
  }, 4000);
}
