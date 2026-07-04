const uploadContainer = document.getElementById('uploadContainer');
const uploadZone = document.getElementById('uploadZone');
const fileInput = document.getElementById('fileInput');
const gifGrid = document.getElementById('gifGrid');
const authBtn = document.getElementById('authBtn');
const toast = document.getElementById('toast');
const uploadContent = document.getElementById('uploadContent');
const uploadLoader = document.getElementById('uploadLoader');
const searchInput = document.getElementById('searchInput');
const uploadTags = document.getElementById('uploadTags');

const uploadPreview = document.getElementById('uploadPreview');
const selectedFileName = document.getElementById('selectedFileName');
const confirmUploadBtn = document.getElementById('confirmUploadBtn');

const loginModal = document.getElementById('loginModal');
const loginForm = document.getElementById('loginForm');
const closeModalBtn = document.getElementById('closeModalBtn');
const masterKey = document.getElementById('masterKey');
const submitLoginBtn = document.getElementById('submitLoginBtn');
const loadMoreTrigger = document.getElementById('loadMoreTrigger');

const nsfwModal = document.getElementById('nsfwModal');
const nsfwAgreeBtn = document.getElementById('nsfwAgreeBtn');
const nsfwToggle = document.getElementById('nsfwToggle');
const uploadNsfwToggleBtn = document.getElementById('uploadNsfwToggleBtn');

let isLoggedIn = false;
let isUploadNsfw = false;
let currentPage = 1;
let currentQuery = '';
let hasMore = true;
let isLoading = false;
let searchTimeout = null;
let pendingFile = null;
let nsfwMode = localStorage.getItem('nsfw_mode_active') === 'true';

if (nsfwToggle) nsfwToggle.checked = nsfwMode;
if (nsfwMode) document.body.classList.add('nsfw-active');

async function checkAuth() {
  try {
    const res = await fetch('/api/auth/status');
    const data = await res.json();
    isLoggedIn = data.loggedIn;
    
    if (isLoggedIn) {
      document.body.classList.add('logged-in');
      if (uploadContainer) uploadContainer.style.display = 'block';
      const suggestContainer = document.getElementById('suggestContainer');
      const reviewContainer = document.getElementById('reviewContainer');
      if (suggestContainer) suggestContainer.style.display = 'none';
      if (reviewContainer) reviewContainer.style.display = 'none';
      const suggestBtn = document.getElementById('suggestNavBtn');
      if (suggestBtn) suggestBtn.textContent = 'review queue';
      if (authBtn) authBtn.textContent = 'Logout';
      localStorage.setItem('nsfw_agreed', 'true');
    } else {
      document.body.classList.remove('logged-in');
      if (uploadContainer) uploadContainer.style.display = 'none';
      const suggestContainer = document.getElementById('suggestContainer');
      const reviewContainer = document.getElementById('reviewContainer');
      if (suggestContainer) suggestContainer.style.display = 'none';
      if (reviewContainer) reviewContainer.style.display = 'none';
      const suggestBtn = document.getElementById('suggestNavBtn');
      if (suggestBtn) suggestBtn.textContent = 'suggest';
      if (authBtn) authBtn.textContent = 'Login';
    }
  } catch (err) {
    console.error(err);
  }
}

const suggestNavBtn = document.getElementById('suggestNavBtn');
if (suggestNavBtn) {
  suggestNavBtn.addEventListener('click', (e) => {
    e.preventDefault();
    const suggestContainer = document.getElementById('suggestContainer');
    const reviewContainer = document.getElementById('reviewContainer');
    
    if (isLoggedIn) {
      if (reviewContainer.style.display === 'none') {
        reviewContainer.style.display = 'block';
        loadSuggestions();
      } else {
        reviewContainer.style.display = 'none';
      }
    } else {
      if (suggestContainer.style.display === 'none') {
        suggestContainer.style.display = 'block';
      } else {
        suggestContainer.style.display = 'none';
      }
    }
  });
}

async function loadGifs(pageNumber = 1) {
  if (pageNumber === true) pageNumber = 1;
  if (isLoading) return;
  isLoading = true;
  currentPage = pageNumber;
  
  if (gifGrid) {
    gifGrid.innerHTML = '';
  }

  try {
    const res = await fetch(`/api/gifs?page=${currentPage}&limit=20&q=${encodeURIComponent(currentQuery)}&nsfw=${nsfwMode}`);
    const data = await res.json();
    renderGifs(data.gifs);
    renderPagination(data.totalPages, data.currentPage);
  } catch (err) {
    showToast('Failed to load library');
  } finally {
    isLoading = false;
  }
}

function renderPagination(totalPages, currentPage) {
  const container = document.getElementById('paginationContainer');
  if (!container) return;
  container.innerHTML = '';
  if (totalPages <= 1) return;

  const prevBtn = document.createElement('button');
  prevBtn.className = 'page-btn';
  prevBtn.textContent = 'Prev';
  prevBtn.disabled = currentPage === 1;
  prevBtn.onclick = () => loadGifs(currentPage - 1);
  container.appendChild(prevBtn);

  let startPage = Math.max(1, currentPage - 2);
  let endPage = Math.min(totalPages, currentPage + 2);

  if (startPage > 1) {
    const firstBtn = document.createElement('button');
    firstBtn.className = 'page-btn';
    firstBtn.textContent = '1';
    firstBtn.onclick = () => loadGifs(1);
    container.appendChild(firstBtn);
    if (startPage > 2) {
      const dots = document.createElement('span');
      dots.textContent = '...';
      dots.style.alignSelf = 'center';
      container.appendChild(dots);
    }
  }

  for (let i = startPage; i <= endPage; i++) {
    const btn = document.createElement('button');
    btn.className = `page-btn ${i === currentPage ? 'active' : ''}`;
    btn.textContent = i;
    btn.onclick = () => loadGifs(i);
    container.appendChild(btn);
  }

  if (endPage < totalPages) {
    if (endPage < totalPages - 1) {
      const dots = document.createElement('span');
      dots.textContent = '...';
      dots.style.alignSelf = 'center';
      container.appendChild(dots);
    }
    const lastBtn = document.createElement('button');
    lastBtn.className = 'page-btn';
    lastBtn.textContent = totalPages;
    lastBtn.onclick = () => loadGifs(totalPages);
    container.appendChild(lastBtn);
  }

  const nextBtn = document.createElement('button');
  nextBtn.className = 'page-btn';
  nextBtn.textContent = 'Next';
  nextBtn.disabled = currentPage === totalPages;
  nextBtn.onclick = () => loadGifs(currentPage + 1);
  container.appendChild(nextBtn);
}

function escapeHTML(str) {
  if (!str) return '';
  return String(str).replace(/[&<>'"]/g, tag => ({
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    "'": '&#39;',
    '"': '&quot;'
  }[tag]));
}

function renderGifs(gifs) {
  if (!gifGrid) return;
  gifs.forEach(gif => {
    const isNsfw = gif.tags && gif.tags.some(t => t.toLowerCase() === 'nsfw');
    const card = document.createElement('div');
    card.className = isNsfw ? 'gif-card nsfw-card' : 'gif-card';
    
    const tagsHtml = gif.tags && gif.tags.length > 0 
      ? `<div class="gif-tags">${gif.tags.map(t => `<span>#${escapeHTML(t)}</span>`).join('')}</div>`
      : '';
      
    const deleteBtnHtml = isLoggedIn 
      ? `<button class="action-btn delete icon-btn" data-key="${escapeHTML(gif.key)}" style="background:var(--danger); color: white;" title="Delete">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="pointer-events:none;"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
         </button>` 
      : '';
      
    const editBtnHtml = isLoggedIn
      ? `<button class="action-btn edit icon-btn" data-key="${escapeHTML(gif.key)}" data-tags="${escapeHTML((gif.tags||[]).join(','))}" title="Edit">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="pointer-events:none;"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path></svg>
         </button>`
      : '';
      
    const slugUrl = gif.slug ? escapeHTML(gif.slug) : escapeHTML(gif.key);

    card.innerHTML = `
      <img src="${escapeHTML(gif.url)}" loading="lazy" alt="GIF" class="gif-img">
      ${tagsHtml}
      <div class="gif-actions">
        <button class="action-btn copy-btn icon-btn" data-key="${escapeHTML(gif.key)}" data-url="${window.location.origin}/gif/${slugUrl}.webp" style="background: white; color: black;" title="Copy Link">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="pointer-events:none;"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
        </button>
        <div style="display:flex; gap:8px;">
          ${editBtnHtml}
          ${deleteBtnHtml}
        </div>
      </div>
    `;
    gifGrid.appendChild(card);
  });
}

if (gifGrid) {
  gifGrid.addEventListener('click', async (e) => {
    const nsfwCard = e.target.closest('.nsfw-card');
    if (nsfwCard && !nsfwMode && !isLoggedIn) {
      showToast('Enable NSFW Mode in the header to view this content');
      if (nsfwToggle) {
        const toggleParent = nsfwToggle.closest('.toggle-switch');
        if (toggleParent) {
          toggleParent.classList.add('shake');
          setTimeout(() => toggleParent.classList.remove('shake'), 400);
        }
      }
      return;
    }
    
    if (e.target.closest('.action-btn')) {
      return; // Let the second listener handle button clicks
    }

    // Toggle mobile overlay
    if (e.target.classList.contains('gif-img') || e.target.classList.contains('gif-card') || e.target.closest('.gif-card')) {
      const card = e.target.closest('.gif-card');
      const wasActive = card.classList.contains('active');
      document.querySelectorAll('.gif-card.active').forEach(c => c.classList.remove('active'));
      
      if (!wasActive) {
        card.classList.add('active');
      }
      return;
    }
  });
}

if (authBtn) {
  authBtn.addEventListener('click', async () => {
    if (isLoggedIn) {
      await fetch('/api/logout', { method: 'POST' });
      isLoggedIn = false;
      document.body.classList.remove('logged-in');
      authBtn.textContent = 'Login';
      if (uploadContainer) uploadContainer.style.display = 'none';
      loadGifs(true); // reload to remove buttons
      showToast('Logged out');
    } else {
      loginModal.style.display = 'flex';
      masterKey.value = '';
      setTimeout(() => masterKey.focus(), 100);
    }
  });
}

if (loginModal) {
  loginModal.addEventListener('click', (e) => {
    if (e.target === loginModal) {
      loginModal.style.display = 'none';
    }
  });
}

if (loginForm) {
  loginForm.addEventListener('submit', async (e) => {
    e.preventDefault();
    const key = masterKey.value;
    const originalText = submitLoginBtn.textContent;
    submitLoginBtn.textContent = 'Verifying...';
    
    try {
      const res = await fetch('/api/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ key })
      });
      
      if (res.ok) {
        loginModal.style.display = 'none';
        isLoggedIn = true;
        document.body.classList.add('logged-in');
        authBtn.textContent = 'Logout';
        if (uploadContainer) uploadContainer.style.display = 'block';
        loadGifs(true); 
        showToast('Logged in');
      } else {
        showToast('Invalid passphrase');
      }
    } catch (err) {
      showToast('Error connecting to server');
    } finally {
      submitLoginBtn.textContent = originalText;
    }
  });
}

// Search
if (searchInput) {
  searchInput.addEventListener('input', (e) => {
    clearTimeout(searchTimeout);
    currentQuery = e.target.value.toLowerCase().trim();
    searchTimeout = setTimeout(() => {
      loadGifs(true);
    }, 300);
  });
}

// Upload Two-Step
if (uploadZone) {
  uploadZone.addEventListener('click', () => fileInput.click());

  uploadZone.addEventListener('dragover', (e) => {
    e.preventDefault();
    uploadZone.classList.add('dragover');
  });

  uploadZone.addEventListener('dragleave', () => {
    uploadZone.classList.remove('dragover');
  });

  uploadZone.addEventListener('drop', (e) => {
    e.preventDefault();
    uploadZone.classList.remove('dragover');
    if (e.dataTransfer.files.length) {
      previewFile(e.dataTransfer.files[0]);
    }
  });

  fileInput.addEventListener('change', (e) => {
    if (e.target.files.length) {
      previewFile(e.target.files[0]);
    }
  });
}

function previewFile(file) {
  const allowed = ['image/gif', 'image/jpeg', 'image/png', 'image/webp', 'video/mp4', 'video/webm', 'video/quicktime'];
  if (!allowed.includes(file.type)) {
    showToast('Invalid file type! Images/GIFs/Videos only.');
    return;
  }
  pendingFile = file;
  selectedFileName.textContent = file.name;
  uploadPreview.style.display = 'block';
}

if (confirmUploadBtn) {
  confirmUploadBtn.addEventListener('click', async () => {
    if (!pendingFile) return;

    const formData = new FormData();
    formData.append('gif', pendingFile);
    if (uploadTags) {
      let tagsVal = uploadTags.value;
      if (isUploadNsfw) {
        tagsVal = tagsVal ? tagsVal + ', nsfw' : 'nsfw';
      }
      formData.append('tags', tagsVal);
    }

    uploadContent.style.display = 'none';
    uploadLoader.style.display = 'block';
    uploadZone.style.pointerEvents = 'none';
    uploadPreview.style.display = 'none';
    confirmUploadBtn.disabled = true;

    try {
      const res = await fetch('/api/upload', {
        method: 'POST',
        body: formData
      });
      
      if (res.ok) {
        showToast('Uploaded successfully');
        if (uploadTags) uploadTags.value = '';
        if (uploadNsfwToggleBtn) {
          isUploadNsfw = false;
          uploadNsfwToggleBtn.classList.remove('active');
        }
        pendingFile = null;
        loadGifs(true);
      } else {
        showToast('Upload failed');
      }
    } catch (err) {
      showToast('Upload error');
    } finally {
      uploadContent.style.display = 'block';
      uploadLoader.style.display = 'none';
      uploadZone.style.pointerEvents = 'auto';
      confirmUploadBtn.disabled = false;
      fileInput.value = '';
    }
  });
}

// Edit, Delete & Copy
if (gifGrid) {
  gifGrid.addEventListener('click', async (e) => {
    if (e.target.closest('.copy-btn')) {
      const btn = e.target.closest('.copy-btn');
      const url = btn.dataset.url;
      try {
        await navigator.clipboard.writeText(url);
        showToast('copied link!');
      } catch (err) {
        const textArea = document.createElement("textarea");
        textArea.value = url;
        document.body.appendChild(textArea);
        textArea.select();
        try {
          document.execCommand('copy');
          showToast('copied link!');
        } catch (ex) {
          showToast('failed to copy');
        }
        document.body.removeChild(textArea);
      }
    }

    if (e.target.closest('.edit')) {
      const btn = e.target.closest('.edit');
      const key = btn.dataset.key;
      const currentTags = btn.dataset.tags;
      
      const newTags = prompt("Edit tags (comma separated):", currentTags);
      if (newTags === null) return; // user cancelled

      const originalText = btn.innerHTML;
      btn.textContent = '...';
      
      try {
        const res = await fetch(`/api/gifs/${key}/tags`, { 
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ tags: newTags })
        });
        if (res.ok) {
          showToast('Tags updated');
          loadGifs(currentPage);
        } else {
          showToast('Update failed');
          btn.textContent = originalText;
        }
      } catch (err) {
        showToast('Update error');
        btn.textContent = originalText;
      }
    }

    if (e.target.closest('.delete')) {
      const btn = e.target.closest('.delete');
      if (!confirm("Are you sure you want to delete this GIF?")) return;

      const key = btn.dataset.key;
      const originalText = btn.innerHTML;
      btn.textContent = '...';
      
      try {
        const res = await fetch(`/api/gifs/${key}`, { method: 'DELETE' });
        if (res.ok) {
          showToast('Deleted');
          loadGifs(currentPage);
        } else {
          showToast('Delete failed');
          btn.textContent = originalText;
        }
      } catch (err) {
        showToast('Delete error');
        btn.textContent = originalText;
      }
    }
  });
}

function showToast(msg) {
  if (!toast) return;
  toast.textContent = msg;
  toast.classList.add('show');
  setTimeout(() => toast.classList.remove('show'), 3000);
}

// Pagination replaces infinite scroll

if (nsfwToggle) {
  nsfwToggle.addEventListener('change', (e) => {
    if (e.target.checked) {
      if (!localStorage.getItem('nsfw_agreed')) {
        e.target.checked = false; // Revert visually
        nsfwModal.style.display = 'flex';
      } else {
        nsfwMode = true;
        localStorage.setItem('nsfw_mode_active', 'true');
        document.body.classList.add('nsfw-active');
        updateNsfwDom();
      }
    } else {
      nsfwMode = false;
      localStorage.setItem('nsfw_mode_active', 'false');
      document.body.classList.remove('nsfw-active');
      updateNsfwDom();
    }
  });
}

async function updateNsfwDom() {
  const cards = document.querySelectorAll('.nsfw-card');
  if (cards.length === 0) return;
  
  if (nsfwMode) {
    cards.forEach(card => {
      const copyBtn = card.querySelector('.copy-btn');
      if (copyBtn) {
        const key = copyBtn.dataset.key;
        const img = card.querySelector('img');
        if (img) img.src = `https://r2.giflibrary.site/${key}`;
      }
    });
  } else {
    try {
      const res = await fetch(`/api/gifs?limit=1000&nsfw=false`);
      const data = await res.json();
      const svgMap = {};
      data.gifs.forEach(g => { svgMap[g.key] = g.url; });
      
      cards.forEach(card => {
        const copyBtn = card.querySelector('.copy-btn');
        if (copyBtn) {
          const key = copyBtn.dataset.key;
          const img = card.querySelector('img');
          if (img && svgMap[key]) {
            img.src = svgMap[key];
          }
        }
      });
    } catch(e) {
      console.error(e);
    }
  }
}

if (nsfwAgreeBtn) {
  nsfwAgreeBtn.addEventListener('click', () => {
    localStorage.setItem('nsfw_agreed', 'true');
    localStorage.setItem('nsfw_mode_active', 'true');
    nsfwModal.style.display = 'none';
    nsfwMode = true;
    if (nsfwToggle) nsfwToggle.checked = true;
    document.body.classList.add('nsfw-active');
    updateNsfwDom();
  });
}

if (uploadNsfwToggleBtn) {
  uploadNsfwToggleBtn.addEventListener('click', () => {
    isUploadNsfw = !isUploadNsfw;
    if (isUploadNsfw) {
      uploadNsfwToggleBtn.classList.add('active');
    } else {
      uploadNsfwToggleBtn.classList.remove('active');
    }
  });
}

const nsfwCancelBtn = document.getElementById('nsfwCancelBtn');

if (nsfwCancelBtn) {
  nsfwCancelBtn.addEventListener('click', (e) => {
    e.preventDefault();
    nsfwModal.style.display = 'none';
    if (nsfwToggle) nsfwToggle.checked = false;
  });
}

checkAuth().then(() => loadGifs(true));

// Suggestion Logic
let pendingSuggestFile = null;
const suggestFileInput = document.getElementById('suggestFileInput');
const suggestZone = document.getElementById('suggestZone');
const suggestPreview = document.getElementById('suggestPreview');
const suggestFileName = document.getElementById('suggestFileName');
const confirmSuggestBtn = document.getElementById('confirmSuggestBtn');
const suggestLoader = document.getElementById('suggestLoader');
const suggestContent = document.getElementById('suggestContent');

if (suggestZone) {
  suggestZone.addEventListener('click', () => suggestFileInput.click());
  suggestZone.addEventListener('dragover', (e) => { e.preventDefault(); suggestZone.classList.add('dragover'); });
  suggestZone.addEventListener('dragleave', () => suggestZone.classList.remove('dragover'));
  suggestZone.addEventListener('drop', (e) => {
    e.preventDefault();
    suggestZone.classList.remove('dragover');
    if (e.dataTransfer.files.length) handleSuggestFile(e.dataTransfer.files[0]);
  });
  suggestFileInput.addEventListener('change', (e) => {
    if (e.target.files.length) handleSuggestFile(e.target.files[0]);
  });
}

function handleSuggestFile(file) {
  const allowed = ['image/gif', 'image/jpeg', 'image/png', 'image/webp', 'video/mp4', 'video/webm', 'video/quicktime'];
  if (!allowed.includes(file.type)) {
    showToast('Invalid file type! Images/GIFs/Videos only.');
    return;
  }
  pendingSuggestFile = file;
  suggestFileName.textContent = file.name;
  suggestPreview.style.display = 'block';
}

if (confirmSuggestBtn) {
  confirmSuggestBtn.addEventListener('click', async () => {
    if (!pendingSuggestFile) return;
    const sender = document.getElementById('suggestSender').value.trim();
    if (!sender) {
      showToast('Name is required!');
      return;
    }
    
    suggestContent.style.display = 'none';
    suggestLoader.style.display = 'block';
    
    const formData = new FormData();
    formData.append('gif', pendingSuggestFile);
    formData.append('tags', document.getElementById('suggestTags').value);
    formData.append('sentBy', sender);
    
    try {
      const res = await fetch('/api/suggest', { method: 'POST', body: formData });
      if (res.ok) {
        showToast('Suggestion submitted successfully!');
        pendingSuggestFile = null;
        suggestPreview.style.display = 'none';
        document.getElementById('suggestSender').value = '';
        document.getElementById('suggestTags').value = '';
      } else {
        const d = await res.json();
        showToast(d.error || 'Failed to submit');
      }
    } catch (e) {
      showToast('Error uploading');
    } finally {
      suggestLoader.style.display = 'none';
      suggestContent.style.display = 'block';
      suggestFileInput.value = '';
    }
  });
}

async function loadSuggestions() {
  const reviewGrid = document.getElementById('reviewGrid');
  if (!reviewGrid) return;
  
  try {
    const res = await fetch('/api/suggestions');
    const data = await res.json();
    reviewGrid.innerHTML = '';
    
    if (data.suggestions.length === 0) {
      reviewGrid.innerHTML = '<p style="color:var(--text-secondary); width: 100%; text-align: center;">No pending suggestions.</p>';
      return;
    }
    
    data.suggestions.forEach(s => {
      const card = document.createElement('div');
      card.className = 'gif-card';
      const tags = s.tags && s.tags.length ? s.tags.map(t => `#${escapeHTML(t)}`).join(' ') : 'No tags';
      card.innerHTML = `
        <img src="${escapeHTML(s.url)}" loading="lazy">
        <div style="padding: 10px; font-size: 13px;">
          <div style="color:var(--text-secondary); margin-bottom: 5px;">Suggested by: <b style="color:white;">${escapeHTML(s.sentBy)}</b></div>
          <div style="margin-bottom: 10px; color: #ccff00;">${tags}</div>
          <div style="display:flex; gap: 8px;">
            <button class="btn btn-primary approve-btn" data-key="${escapeHTML(s.key)}" style="flex:1; padding: 6px; font-size:12px; background: #ccff00; color: #000;">Approve</button>
            <button class="btn btn-primary reject-btn" data-key="${escapeHTML(s.key)}" style="flex:1; padding: 6px; font-size:12px; background: var(--danger); border: none;">Reject</button>
          </div>
        </div>
      `;
      reviewGrid.appendChild(card);
    });
    
    reviewGrid.querySelectorAll('.approve-btn').forEach(btn => {
      btn.addEventListener('click', async (e) => {
        const key = e.target.dataset.key;
        e.target.textContent = '...';
        await fetch(`/api/suggestions/${key}/approve`, { method: 'POST' });
        showToast('Approved!');
        loadSuggestions();
        loadGifs(true);
      });
    });
    
    reviewGrid.querySelectorAll('.reject-btn').forEach(btn => {
      btn.addEventListener('click', async (e) => {
        const key = e.target.dataset.key;
        e.target.textContent = '...';
        await fetch(`/api/suggestions/${key}/reject`, { method: 'DELETE' });
        showToast('Rejected!');
        loadSuggestions();
      });
    });
    
  } catch (e) {
    console.error(e);
  }
}

document.body.addEventListener('input', (e) => {
  if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') {
    const val = e.target.value;
    const trollRegex = /<\s*script|drop\s+table|select\s+.*\s+from|insert\s+into|javascript:|1\s*=\s*1|union\s+select|<iframe|<object|<embed|<svg|onerror=|onload=|eval\(/i;
    if (trollRegex.test(val)) {
      showToast("fucj u stop it");
      if (!e.target.dataset.trolled) {
        e.target.dataset.trolled = "true";
        fetch('/api/troll', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({ payload: val })
        }).catch(()=>{});
      }
    } else {
      e.target.dataset.trolled = "";
    }
  }
});
