const API_BASE_URL = 'http://localhost:3000';
const API_KEY = 'a-very-secret-api-key-change-me'; // <-- IMPORTANT: Replace with your actual API key

let allLeads = [];
let paginationInfo = {};
let currentView = 'new';
let currentPage = 1;
let leadsPerPage = 50;
let selectedLeads = new Set();
let currentSearchTerm = '';
let currentValidationFilter = '';
let currentDisputeAction = '';
let enhancedStats = null;
let jobPollingInterval = null; // To hold our polling timer
let isInitialized = false; // Flag to track initialization

// ================== ENHANCED DATA REFRESH SYSTEM ==================

function escapeHTML(str) {
    if (str === null || str === undefined) {
        return '';
    }
    return str.toString()
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#039;');
}

function createCell(text) {
    const td = document.createElement('td');
    td.textContent = text || '';
    return td;
}
// Enhanced data refresh function with retry logic
async function refreshAllData(retries = 3) {
    console.log('Refreshing all data...');
    
    for (let attempt = 1; attempt <= retries; attempt++) {
        try {
            // Add a small delay to ensure backend operations are complete
            await new Promise(resolve => setTimeout(resolve, 500));
            
            // Fetch both stats and leads data
            await Promise.all([
                fetchEnhancedStats(),
                fetchLeads()
            ]);
            
            console.log('Data refresh completed successfully');
            return;
        } catch (error) {
            console.error(`Data refresh attempt ${attempt} failed:`, error);
            if (attempt === retries) {
                showNotification('Failed to refresh data. Please refresh the page.', 'error');
                throw error;
            }
            // Wait before retrying
            await new Promise(resolve => setTimeout(resolve, 1000));
        }
    }
}

// Enhanced operation wrapper that ensures data refresh
async function performOperation(operationName, operationFn) {
    console.log(`Starting operation: ${operationName}`);
    
    try {
        // Perform the operation
        await operationFn();
        
        // Always refresh data after any operation
        await refreshAllData();
        
        console.log(`Operation completed: ${operationName}`);
    } catch (error) {
        console.error(`Operation failed: ${operationName}`, error);
        
        // Even if operation failed, try to refresh data to get current state
        try {
            await refreshAllData();
        } catch (refreshError) {
            console.error('Failed to refresh data after operation failure', refreshError);
        }
        
        throw error;
    }
}

// Clear cached data function
function clearCachedData() {
    allLeads = [];
    selectedLeads.clear();
    enhancedStats = null;
    console.log('Cached data cleared');
}

// ================== ENHANCED API AND EVENT FUNCTIONS ==================

document.addEventListener('DOMContentLoaded', function() {
    console.log('Application initializing...');
    
    // Set up event listeners
    setupEventListeners();
    
    // Mark as initialized
    isInitialized = true;
    
    // Initial data load with retry - delay slightly to ensure DOM is ready
    setTimeout(async () => {
        try {
            await refreshAllData();
        } catch (error) {
            console.error('Initial data load failed:', error);
            showNotification('Failed to load initial data. Please refresh the page.', 'error');
        }
    }, 100);
});

function setupEventListeners() {
    document.getElementById('searchInput').addEventListener('input', debounce(handleSearch, 300));
    document.getElementById('intervalSeconds')?.addEventListener('input', updateEstimatedTime);
    
    ['emailModal', 'zapierModal', 'disputeModal', 'bulkResolveModal', 'importModal'].forEach(modalId => {
        const modal = document.getElementById(modalId);
        modal?.addEventListener('click', (e) => {
            if (e.target === modal) closeModal(modalId);
        });
    });
}

function closeModal(modalId) {
    document.getElementById(modalId).style.display = 'none';
}

// Enhanced apiFetch with cache-busting
async function apiFetch(endpoint, options = {}) {
    const defaultOptions = {
        headers: {
            'Authorization': `Bearer ${API_KEY}`,
            'Accept': 'application/json',
            'Cache-Control': 'no-cache',
            'Pragma': 'no-cache'
        }
    };
    
    if (options.body && !(options.body instanceof FormData)) {
        defaultOptions.headers['Content-Type'] = 'application/json';
    }

    const mergedOptions = { ...defaultOptions, ...options };
    mergedOptions.headers = { ...defaultOptions.headers, ...options.headers };
    
    // Add cache-busting timestamp for GET requests
    let finalEndpoint = endpoint;
    if (!options.method || options.method === 'GET') {
        const separator = endpoint.includes('?') ? '&' : '?';
        finalEndpoint = `${endpoint}${separator}_t=${Date.now()}`;
    }
    
    try {
        const response = await fetch(`${API_BASE_URL}${finalEndpoint}`, mergedOptions);
        if (!response.ok) {
            const errorData = await response.json().catch(() => ({ error: { message: 'An unknown error occurred' } }));
            throw new Error(errorData.error.message || `HTTP error! status: ${response.status}`);
        }
        return response;
    } catch (error) {
        console.error('API call failed:', error);
        showNotification(error.message, 'error');
        throw error;
    }
}

// Enhanced fetchLeads function
async function fetchLeads() {
    try {
        console.log('Fetching leads for view:', currentView);

        let endpoint = `/api/leads?status=${currentView}&search=${currentSearchTerm}&page=${currentPage}&per_page=${leadsPerPage}`;

        if (currentValidationFilter) {
            endpoint += `&validation_result=${currentValidationFilter}`;
        }

        updateTableHeaders(); // Ensure headers are correct for the current view

        if (currentView === 'disputed') {
            endpoint = `/api/disputes?page=${currentPage}&per_page=${leadsPerPage}`;
            const response = await apiFetch(endpoint);
            const data = await response.json();
            // In disputed view, data is nested differently
            const leadsData = Array.isArray(data.data) ? data.data : []; 
            allLeads = leadsData.map(item => ({...item.lead, conflicts: item.conflicts, validations: item.validations}));
            paginationInfo = data.pagination;
            renderDisputedLeads(allLeads);
        } else {
            const response = await apiFetch(endpoint);
            const data = await response.json();
            allLeads = Array.isArray(data.data) ? data.data : []; 
            paginationInfo = data.pagination;
            renderLeads();
        }

        updateButtons();
        console.log('Leads fetched successfully:', allLeads.length, 'leads');
    } catch (error) {
        console.error('Failed to fetch leads:', error);
    }
}

// Enhanced fetchEnhancedStats function
async function fetchEnhancedStats() {
    try {
        console.log('Fetching enhanced stats...');
        const response = await apiFetch('/api/leads/stats');
        enhancedStats = await response.json();
        updateTabCounts();
        updateStatsDisplay();
        console.log('Stats fetched successfully');
    } catch (error) {
        console.error('Failed to fetch enhanced stats:', error);
        showNotification('Failed to load statistics.', 'warning');
    }
}

function updateTabCounts() {
    if (enhancedStats) {
        document.getElementById('newCount').textContent = enhancedStats.new_leads;
        document.getElementById('contactedCount').textContent = enhancedStats.contacted_leads;
        document.getElementById('disputedCount').textContent = enhancedStats.disputed_leads;
        
        const disputeBadge = document.getElementById('disputeBadge');
        if (enhancedStats.disputed_leads > 0) {
            disputeBadge.style.display = 'block';
        } else {
            disputeBadge.style.display = 'none';
        }
    }
}

function updateStatsDisplay() {
    const statsGrid = document.getElementById('statsGrid');
    const validationServiceInfo = document.getElementById('validationServiceInfo');
    
    if (currentView === 'disputed' && enhancedStats) {
        statsGrid.style.display = 'grid';
        validationServiceInfo.style.display = 'block';
        
        // Enhanced stats with validation service information
        statsGrid.innerHTML = `
            <div class="stat-card">
                <div class="stat-number" style="color: #d69e2e;">${enhancedStats.dispute_stats.total_disputed}</div>
                <div class="stat-label">Total Disputed</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #ed8936;">${enhancedStats.dispute_stats.same_domain_conflicts}</div>
                <div class="stat-label">Same Domain</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #4338ca;">${enhancedStats.dispute_stats.contacted_domain_conflicts}</div>
                <div class="stat-label">Contacted Domain</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #e53e3e;">${enhancedStats.dispute_stats.duplicate_email_conflicts}</div>
                <div class="stat-label">Duplicate Email</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #d53f8c;">${enhancedStats.dispute_stats.similar_name_conflicts}</div>
                <div class="stat-label">Similar Name</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #38a169;">${enhancedStats.validation_stats.valid_emails}</div>
                <div class="stat-label">Valid Emails</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #e53e3e;">${enhancedStats.validation_stats.invalid_emails}</div>
                <div class="stat-label">Invalid Emails</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #2b8a3e;">${enhancedStats.validation_stats.quickemail_used || 0}</div>
                <div class="stat-label">QuickEmail Used</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #d69e2e;">${enhancedStats.validation_stats.myemailverifier_used || 0}</div>
                <div class="stat-label">MyEmailVerifier Used</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #dc2626;">${enhancedStats.validation_stats.fallbacks_used || 0}</div>
                <div class="stat-label">Fallbacks Used</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" style="color: #991b1b;">${enhancedStats.validation_stats.no_credits_failures || 0}</div>
                <div class="stat-label">Credit Failures</div>
            </div>
        `;
    } else {
        statsGrid.style.display = 'none';
        validationServiceInfo.style.display = 'none';
    }
}

function updateTableHeaders() {
    const tableHeader = document.getElementById('tableHeader');
    if (currentView === 'disputed') {
        tableHeader.innerHTML = `
            <tr>
                <th width="50">
                    <input type="checkbox" id="selectAll" onchange="toggleSelectAll()">
                </th>
                <th>Lead Details</th>
                <th>Conflicts</th>
                <th>Validation & Service</th>
                <th width="180">Actions</th>
            </tr>
        `;
    } else {
        tableHeader.innerHTML = `
            <tr>
                <th width="50">
                    <input type="checkbox" id="selectAll" onchange="toggleSelectAll()">
                </th>
                <th>Name</th>
                <th>Email</th>
                <th>Status</th>
                <th>Company</th>
                <th width="180">Actions</th>
            </tr>
        `;
    }
}

function buildValidationBadge(validation) {
    if (!validation) return '';

    let validationBadge = `<div class="validation-indicator validation-${validation.result}">${validation.result}</div>`;
    let serviceBadge = '';

    if (validation.service) {
        const serviceClass = validation.service === 'QuickEmailVerification' ? 'service-quickemail' :
                             validation.service === 'MyEmailVerifier' ? 'service-myemailverifier' : 'service-fallback';
        serviceBadge = `<div class="service-indicator ${serviceClass}" title="${escapeHTML(validation.details)}">${escapeHTML(validation.service.replace('Verification',''))}</div>`;
    }
    if (validation.credit_status) {
        if (validation.credit_status === 'fallback_used') {
            serviceBadge += '<div class="service-indicator service-fallback">Fallback</div>';
        } else if (validation.credit_status === 'no_credits') {
            serviceBadge += '<div class="service-indicator service-no-credits">No Credits</div>';
        }
    }
    return `<div class="validation-details">${validationBadge}${serviceBadge}</div>`;
}


async function validateSingleEmail(leadId, btn) {
    const originalText = btn.innerHTML;
    btn.innerHTML = '<span class="loading-small"></span> Checking...';
    btn.disabled = true;

    try {
        await apiFetch(`/api/leads/${leadId}/validate`, { method: 'POST' });
        showNotification(`Validation triggered for lead ${leadId}. Refreshing...`, 'success');
        // A simple refresh is the most reliable way to update the UI
        await fetchLeads();
    } catch (error) {
        showNotification(`Validation failed: ${error.message}`, 'error');
        // Restore button only on failure
        btn.innerHTML = originalText;
        btn.disabled = false;
    }
}

// Enhanced switchView function
function switchView(view) {
    console.log('Switching to view:', view);
    
    currentView = view;
    currentPage = 1;
    currentSearchTerm = '';
    currentValidationFilter = '';
    
    document.getElementById('searchInput').value = '';
    document.getElementById('validationFilter').value = '';
    
    clearCachedData();
    
    document.querySelectorAll('.tab').forEach(tab => tab.classList.remove('active'));
    document.getElementById(view + 'Tab').classList.add('active');
    
    const disputeActions = document.getElementById('disputeActions');
    const validationFilter = document.getElementById('validationFilter');
    const sendEmailBtn = document.getElementById('sendEmailBtn');
    const sendZapierBtn = document.getElementById('sendZapierBtn');
    const markContactedBtn = document.getElementById('markContactedBtn');

    
    if (view === 'disputed') {
        disputeActions.style.display = 'flex';
        validationFilter.style.display = 'block';
        sendEmailBtn.style.display = 'none';
        sendZapierBtn.style.display = 'none';
        markContactedBtn.style.display = 'none';
    } else {
        disputeActions.style.display = 'none';
        validationFilter.style.display = 'none';
        sendEmailBtn.style.display = 'inline-flex';
        sendZapierBtn.style.display = 'inline-flex';
        markContactedBtn.style.display = 'inline-flex';
    }
    
    updateTableHeaders(); // This line is correctly placed here.
    updateStatsDisplay();
    refreshAllData();
}

function handleSearch(event) {
    currentSearchTerm = event.target.value;
    currentPage = 1;
    fetchLeads();
}

function handleFilterChange() {
    currentValidationFilter = document.getElementById('validationFilter').value;
    currentPage = 1;
    fetchLeads();
}

function renderLeads() {
    const tbody = document.getElementById('leadsTableBody');
    const emptyState = document.getElementById('emptyState');
    const table = document.querySelector('.table-container table');
    const paginationEl = document.getElementById('pagination');

    tbody.innerHTML = ''; // Clear previous content

    if (allLeads.length === 0) {
        emptyState.style.display = 'block';
        table.style.display = 'none';
        paginationEl.style.display = 'none';
        emptyState.querySelector('h3').textContent = 'No leads found';
        emptyState.querySelector('p').textContent = currentSearchTerm ? `No results for "${currentSearchTerm}"` : `No ${currentView} leads. Try importing some!`;
        return;
    }

    emptyState.style.display = 'none';
    table.style.display = 'table';
    paginationEl.style.display = 'flex';

    allLeads.forEach(lead => {
        const tr = document.createElement('tr');
        
        // Checkbox column
        const tdCheckbox = document.createElement('td');
        const checkbox = document.createElement('input');
        checkbox.type = 'checkbox';
        checkbox.checked = selectedLeads.has(lead.id);
        checkbox.onchange = () => toggleLead(lead.id);
        tdCheckbox.appendChild(checkbox);
        tr.appendChild(tdCheckbox);
        
        // Name column
        const tdName = createCell(lead.name);
        tr.appendChild(tdName);
        
        // Email column
        const tdEmail = createCell(lead.email);
        tr.appendChild(tdEmail);
        
        // Status column
        const tdStatus = document.createElement('td');
        tdStatus.innerHTML = `<span class="status-badge status-${escapeHTML(lead.status)}">${escapeHTML(lead.status.charAt(0).toUpperCase() + lead.status.slice(1))}</span>`;
        tr.appendChild(tdStatus);
        
        // Company column
        const tdCompany = createCell(lead.company || '-');
        tr.appendChild(tdCompany);
        
        // Actions column - FIXED TO ENSURE IT'S ALWAYS RENDERED
        const tdActions = document.createElement('td');
        tdActions.className = 'actions-cell';
        
        let actionsHTML = '';
        
        // Validation button or status
        if (!lead.validation || lead.validation.result === 'unknown') {
            actionsHTML += `<button class="btn btn-secondary btn-sm" onclick="validateSingleEmail(${lead.id}, this)">📧 Check</button>`;
        } else {
            actionsHTML += buildValidationBadge(lead.validation);
        }
        
        // Delete button
        actionsHTML += `<button class="btn btn-danger btn-sm" onclick="deleteLead(${lead.id})">🗑️ Delete</button>`;
        
        tdActions.innerHTML = actionsHTML;
        tr.appendChild(tdActions);
        
        tbody.appendChild(tr);
    });

    renderPagination();
    updateSelectAllCheckbox();
}

async function deleteLead(leadId) {
    if (!confirm(`Are you sure you want to delete lead ${leadId}? This action cannot be undone.`)) {
        return;
    }

    try {
        await performOperation('deleteLead', async () => {
            await apiFetch(`/api/leads/${leadId}`, { method: 'DELETE' });
            showNotification(`Lead ${leadId} has been successfully deleted.`, 'success');
        });
    } catch (error) {
        console.error(`Failed to delete lead ${leadId}:`, error);
        showNotification(`Error deleting lead: ${error.message}`, 'error');
    }
}

function renderDisputedLeads(leadsWithDetails) {
    const tbody = document.getElementById('leadsTableBody');
    const emptyState = document.getElementById('emptyState');
    const table = document.querySelector('.table-container table');
    const paginationEl = document.getElementById('pagination');
    
    tbody.innerHTML = ''; // Clear previous content

    if (leadsWithDetails.length === 0) {
        emptyState.style.display = 'block';
        table.style.display = 'none';
        paginationEl.style.display = 'none';
        
        const emptyTitle = emptyState.querySelector('h3');
        const emptyText = emptyState.querySelector('p');
        emptyTitle.textContent = 'No disputed leads found';
        emptyText.textContent = 'All conflicts have been resolved!';
        
        return;
    }

    emptyState.style.display = 'none';
    table.style.display = 'table';
    paginationEl.style.display = 'flex';

    leadsWithDetails.forEach(item => {
        const tr = document.createElement('tr');
        
        const lead = item; // In disputed view, the item is the enriched lead
        const conflicts = item.conflicts || [];
        const validations = item.validations || [];
        
        const tdCheckbox = document.createElement('td');
        const checkbox = document.createElement('input');
        checkbox.type = 'checkbox';
        checkbox.checked = selectedLeads.has(lead.id);
        checkbox.onchange = () => toggleLead(lead.id);
        tdCheckbox.appendChild(checkbox);
        tr.appendChild(tdCheckbox);
        
        const tdLeadDetails = document.createElement('td');
        tdLeadDetails.innerHTML = `
            <div><strong>${escapeHTML(lead.name)}</strong></div>
            <div style="color: #718096; font-size: 14px;">${escapeHTML(lead.email)}</div>
            <div style="color: #718096; font-size: 12px; margin-top: 5px;">
                Company: ${escapeHTML(lead.company || 'Not specified')}
            </div>`;
        tr.appendChild(tdLeadDetails);
        
        const tdConflicts = document.createElement('td');
        const conflictIndicators = conflicts.map(c => `<div class="conflict-badge conflict-${c.conflict_type.replace(/_/g, '-')}">${c.conflict_type.replace(/_/g, ' ')}</div>`).join('');
        const conflictDetails = conflicts.map(c => `<div class="conflict-item"><strong>${c.conflict_type.replace(/_/g, ' ')}:</strong> ${escapeHTML(c.conflict_details)}</div>`).join('');
        tdConflicts.innerHTML = `
            <div class="conflict-indicators">${conflictIndicators}</div>
            ${conflicts.length > 0 ? `<div class="conflict-details"><h4>Conflict Details:</h4>${conflictDetails}</div>` : ''}
        `;
        tr.appendChild(tdConflicts);
        
        const tdValidation = document.createElement('td');
        const latestValidation = validations[0];
        tdValidation.innerHTML = buildValidationBadge(latestValidation);
        if (latestValidation && latestValidation.details) {
            tdValidation.innerHTML += `<div style="font-size: 12px; color: #718096; margin-top: 5px;">${escapeHTML(latestValidation.details)}</div>`;
        }
         if (!latestValidation) {
            tdValidation.innerHTML = '<div class="validation-indicator validation-unknown">Not Validated</div>'
        }
        tr.appendChild(tdValidation);

        const tdDisputeActions = document.createElement('td');
        tdDisputeActions.className = 'actions-cell';
        tdDisputeActions.innerHTML = `
            <button class="btn btn-success btn-sm" onclick="resolveIndividualDispute(${lead.id}, 'accept')">✅ Accept</button>
            <button class="btn btn-danger btn-sm" onclick="resolveIndividualDispute(${lead.id}, 'discard')">🗑️ Discard</button>
            <button class="btn btn-warning btn-sm" onclick="resolveIndividualDispute(${lead.id}, 'mark_contacted')">📞 Contacted</button>`;
        tr.appendChild(tdDisputeActions);

        tbody.appendChild(tr);
    });

    renderPagination();
    updateSelectAllCheckbox();
}

function renderPagination() {
    const pagination = document.getElementById('pagination');
    const { current_page, total_pages } = paginationInfo;
    
    if (total_pages <= 1) {
        pagination.innerHTML = '';
        return;
    }

    let paginationHTML = '';
    
    if (current_page > 1) {
        paginationHTML += `<button class="page-btn" onclick="changePage(${current_page - 1})">‹ Previous</button>`;
    }

    const startPage = Math.max(1, current_page - 2);
    const endPage = Math.min(total_pages, current_page + 2);

    for (let i = startPage; i <= endPage; i++) {
        paginationHTML += `<button class="page-btn ${i === current_page ? 'active' : ''}" onclick="changePage(${i})">${i}</button>`;
    }

    if (current_page < total_pages) {
        paginationHTML += `<button class="page-btn" onclick="changePage(${current_page + 1})">Next ›</button>`;
    }

    pagination.innerHTML = paginationHTML;
}

function changePage(page) {
    currentPage = page;
    fetchLeads();
}

function toggleLead(leadId) {
    if (selectedLeads.has(leadId)) {
        selectedLeads.delete(leadId);
    } else {
        selectedLeads.add(leadId);
    }
    updateButtons();
    updateSelectAllCheckbox();
}

async function toggleSelectAll() {
    const selectAllCheckbox = document.getElementById('selectAll');
    if (selectAllCheckbox.checked) {
        allLeads.forEach(lead => selectedLeads.add(lead.id));
    } else {
        selectedLeads.clear();
    }
    
    // Re-render to update checkboxes
    if (currentView === 'disputed') {
        renderDisputedLeads(allLeads);
    } else {
        renderLeads();
    }
    updateButtons();
}

function updateSelectAllCheckbox() {
    const selectAllCheckbox = document.getElementById('selectAll');
    // Add null check to prevent errors
    if (!selectAllCheckbox) {
        console.warn('selectAll checkbox not found');
        return;
    }
    
    if (allLeads.length === 0) {
        selectAllCheckbox.checked = false;
        selectAllCheckbox.indeterminate = false;
        return;
    }
    const selectedCount = allLeads.filter(lead => selectedLeads.has(lead.id)).length;
    if (selectedCount === 0) {
        selectAllCheckbox.checked = false;
        selectAllCheckbox.indeterminate = false;
    } else if (selectedCount === allLeads.length) {
        selectAllCheckbox.checked = true;
        selectAllCheckbox.indeterminate = false;
    } else {
        selectAllCheckbox.checked = false;
        selectAllCheckbox.indeterminate = true;
    }
}

function updateButtons() {
    const sendEmailBtn = document.getElementById('sendEmailBtn');
    const sendZapierBtn = document.getElementById('sendZapierBtn');
    const exportBtn = document.getElementById('exportBtn');
    const markContactedBtn = document.getElementById('markContactedBtn');
    const bulkAcceptBtn = document.getElementById('bulkAcceptBtn');
    const bulkDiscardBtn = document.getElementById('bulkDiscardBtn');

    const hasSelected = selectedLeads.size > 0;
    const hasLeads = allLeads.length > 0;

    markContactedBtn.disabled = !hasSelected;
    exportBtn.disabled = !hasLeads;

    if (currentView === 'disputed') {
        if (bulkAcceptBtn) bulkAcceptBtn.disabled = !hasSelected;
        if (bulkDiscardBtn) bulkDiscardBtn.disabled = !hasSelected;
    } else {
        sendEmailBtn.disabled = !hasSelected;
        sendZapierBtn.disabled = !hasSelected;
    }
}

// ================== ENHANCED OPERATION FUNCTIONS ==================

// Enhanced markAsContacted function
async function markAsContacted() {
    if (selectedLeads.size === 0) return;

    await performOperation('markAsContacted', async () => {
        await apiFetch('/api/leads/bulk-update', {
            method: 'POST',
            body: JSON.stringify({
                lead_ids: Array.from(selectedLeads),
                status: 'contacted'
            }),
        });
        
        showNotification(`${selectedLeads.size} leads marked as contacted`, 'success');
        selectedLeads.clear();
    });
}

// Enhanced dispute resolution functions
async function resolveIndividualDispute(leadId, action) {
    await performOperation(`resolveIndividualDispute-${action}`, async () => {
        const response = await apiFetch(`/api/disputes/${leadId}/resolve`, {
            method: 'POST',
            body: JSON.stringify({
                resolution: action,
                notes: `Resolved via individual action: ${action}`
            }),
        });
        
        const result = await response.json();
        showNotification(`Dispute resolved: ${result.status}`, 'success');
    });
}

function openBulkResolveModal(action) {
    if (selectedLeads.size === 0) {
        showNotification('Please select leads to resolve.', 'error');
        return;
    }

    currentDisputeAction = action;
    const modal = document.getElementById('bulkResolveModal');
    const title = document.getElementById('bulkResolveTitle');
    const description = document.getElementById('bulkResolveDescription');

    const actionText = {
        'accept': 'Accept as New Leads',
        'discard': 'Discard',
        'mark_contacted': 'Mark as Contacted'
    };

    title.textContent = `Bulk ${actionText[action]}`;
    description.textContent = `Are you sure you want to ${actionText[action].toLowerCase()} ${selectedLeads.size} disputed lead(s)?`;

    modal.style.display = 'block';
}

function closeBulkResolveModal() {
    document.getElementById('bulkResolveModal').style.display = 'none';
    document.getElementById('bulkResolutionNotes').value = '';
}

// Enhanced confirmBulkResolve function
async function confirmBulkResolve() {
    const notes = document.getElementById('bulkResolutionNotes').value;
    const btn = document.querySelector('#bulkResolveModal .btn-primary');
    
    setButtonLoading(btn, true);

    try {
        await performOperation('confirmBulkResolve', async () => {
            const response = await apiFetch('/api/disputes/bulk-resolve', {
                method: 'POST',
                body: JSON.stringify({
                    lead_ids: Array.from(selectedLeads),
                    resolution: currentDisputeAction,
                    notes: notes || `Bulk resolution: ${currentDisputeAction}`
                }),
            });
            
            const result = await response.json();
            showNotification(`Bulk resolution completed: ${result.resolved_count} disputes resolved`, 'success');
            
            if (result.errors && result.errors.length > 0) {
                showNotification(`Some errors occurred: ${result.errors.length} failed`, 'warning');
            }
            
            selectedLeads.clear();
            closeBulkResolveModal();
        });
    } finally {
        setButtonLoading(btn, false);
    }
}

// Enhanced analyzeConflicts function
async function analyzeConflicts() {
    const btn = document.getElementById('analyzeConflictsBtn');
    const originalText = btn.innerHTML;
    btn.innerHTML = '<span class="loading"></span> Analyzing...';
    btn.disabled = true;

    try {
        await performOperation('analyzeConflicts', async () => {
            const response = await apiFetch('/api/disputes/analyze', { method: 'POST' });
            const result = await response.json();
            
            showNotification(
                `Analysis complete: ${result.conflicts_detected} conflicts detected, ${result.leads_marked_disputed} leads marked as disputed`,
                'success'
            );
            
            if (result.conflict_types && result.conflict_types.length > 0) {
                const details = result.conflict_types.map(ct => `${ct.count} ${ct.conflict_type}`).join(', ');
                showNotification(`Conflict types: ${details}`, 'warning');
            }
        });
    } finally {
        btn.innerHTML = originalText;
        btn.disabled = false;
    }
}

// Enhanced validateAllEmails function
async function validateAllEmails() {
    const btn = document.getElementById('validateEmailsBtn');
    const originalText = btn.innerHTML;
    btn.innerHTML = '<span class="loading"></span> Validating...';
    btn.disabled = true;

    try {
        await performOperation('validateAllEmails', async () => {
            const response = await apiFetch('/api/disputes/validate-emails', { method: 'POST' });
            const result = await response.json();
            
            showNotification(
                `Email validation complete: ${result.validations_performed} emails processed`,
                'success'
            );
            
            // Enhanced notification with service usage details
            if (result.quickemail_validations || result.myemailverifier_validations) {
                showNotification(
                    `Service usage: ${result.quickemail_validations || 0} QuickEmail, ${result.myemailverifier_validations || 0} MyEmailVerifier, ${result.fallbacks_used || 0} fallbacks, ${result.credit_failures || 0} credit failures`,
                    'warning'
                );
            }
        });
    } finally {
        btn.innerHTML = originalText;
        btn.disabled = false;
    }
}

// ================== IMPORT FUNCTIONALITY ==================

function triggerFileUpload() {
    document.getElementById('importModal').style.display = 'block';
}

function closeImportModal() {
    document.getElementById('importModal').style.display = 'none';
    document.getElementById('csvFile').value = '';
}

async function startImport() {
    const fileInput = document.getElementById('csvFile');
    const file = fileInput.files[0];
    
    if (!file) {
        showNotification('Please select a CSV file.', 'error');
        return;
    }

    const importBtn = document.querySelector('#importModal .btn-primary');
    setButtonLoading(importBtn, true);

    const formData = new FormData();
    formData.append('file', file);
    formData.append('enable_validation', document.getElementById('enableValidation').checked);
    formData.append('enable_conflict_detection', document.getElementById('enableConflictDetection').checked);
    formData.append('auto_mark_disputed', document.getElementById('autoMarkDisputed').checked);
    
    try {
        const response = await apiFetch('/api/leads/import', { method: 'POST', body: formData });
        const result = await response.json();
        
        closeImportModal();
        showNotification(`Import job #${result.job_id} started successfully!`, 'success');
        pollJobStatus(result.job_id);

    } catch (error) {
        console.error('Import failed to start:', error);
    } finally {
        setButtonLoading(importBtn, false);
    }
}

// Enhanced pollJobStatus with better refresh
function pollJobStatus(jobId) {
    const progressContainer = document.getElementById('import-progress-container');
    const progressText = document.getElementById('import-progress-text');
    const progressBar = document.getElementById('import-progress-bar');
    
    progressContainer.style.display = 'block';

    if (jobPollingInterval) clearInterval(jobPollingInterval);

    jobPollingInterval = setInterval(async () => {
        try {
            const response = await apiFetch(`/api/jobs/import/${jobId}`);
            const job = await response.json();

            // Update UI with progress
            const percentage = job.total_rows > 0 ? (job.processed_rows / job.total_rows) * 100 : 0;
            progressText.textContent = `Status: ${job.status} (${job.processed_rows} / ${job.total_rows} rows)`;
            progressBar.style.width = `${Math.round(percentage)}%`;

            // Check if the job is finished
            if (job.status === 'completed' || job.status === 'failed') {
                clearInterval(jobPollingInterval);
                jobPollingInterval = null;

                const finalMessage = job.status === 'completed'
                    ? `Import job #${job.id} completed! Processed: ${job.processed_rows} rows, Conflicts: ${job.conflicts_detected}, Disputed: ${job.leads_marked_disputed}, Validated: ${job.emails_validated}`
                    : `Import job #${job.id} failed: ${job.error}`;
                const finalType = job.status === 'completed' ? 'success' : 'error';
                showNotification(finalMessage, finalType);

                // Show enhanced statistics for completed jobs
                if (job.status === 'completed') {
                    setTimeout(() => {
                        showNotification(
                            `Service usage: ${job.quickemail_validations || 0} QuickEmail, ${job.myemailverifier_validations || 0} MyEmailVerifier, ${job.validation_fallbacks || 0} fallbacks, ${job.validation_credit_failures || 0} credit failures`,
                            'warning'
                        );
                    }, 2000);
                }

                // Hide progress bar after a few seconds and refresh data
                setTimeout(() => {
                    progressContainer.style.display = 'none';
                }, 5000);
                
                // Use enhanced refresh with longer delay for import completion
                setTimeout(async () => {
                    await refreshAllData();
                }, 1000);
            }
        } catch (error) {
            console.error('Failed to poll job status:', error);
            showNotification('Lost connection to import job status.', 'error');
            clearInterval(jobPollingInterval);
            progressContainer.style.display = 'none';
        }
    }, 2000);
}

function setButtonLoading(btn, isLoading) {
    const textSpan = btn.querySelector('span:not(.loading)');
    const loadingSpan = btn.querySelector('.loading');
    if (isLoading) {
        if(textSpan) textSpan.style.display = 'none';
        if(loadingSpan) loadingSpan.style.display = 'inline-block';
        btn.disabled = true;
    } else {
        if(textSpan) textSpan.style.display = 'inline';
        if(loadingSpan) loadingSpan.style.display = 'none';
        btn.disabled = false;
    }
}

// ================== EMAIL FUNCTIONALITY ==================

function openEmailModal() {
    document.getElementById('emailModal').style.display = 'block';
    document.getElementById('emailSubject').focus();
}

function closeEmailModal() {
    document.getElementById('emailModal').style.display = 'none';
    document.getElementById('emailSubject').value = '';
    document.getElementById('emailBody').value = '';
}

function openZapierModal() {
    document.getElementById('zapierModal').style.display = 'block';
    updateEstimatedTime();
    document.getElementById('intervalSeconds').focus();
}

function closeZapierModal() {
    document.getElementById('zapierModal').style.display = 'none';
}

function updateEstimatedTime() {
    const interval = parseInt(document.getElementById('intervalSeconds').value) || 10;
    const selectedCount = selectedLeads.size;
    
    if (selectedCount === 0) {
        document.getElementById('estimatedTimeText').textContent = 'No leads selected';
        return;
    }

    const totalSeconds = (selectedCount - 1) * interval;
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    
    let timeText = '';
    if (minutes > 0) {
        timeText = `${minutes}m ${seconds}s`;
    } else {
        timeText = `${seconds}s`;
    }
    
    document.getElementById('estimatedTimeText').textContent = 
        `${timeText} for ${selectedCount} lead${selectedCount > 1 ? 's' : ''}`;
}

// Enhanced sendEmails function
async function sendEmails() {
    const subject = document.getElementById('emailSubject').value.trim();
    const body = document.getElementById('emailBody').value.trim();

    if (!subject || !body) {
        showNotification('Please fill in both subject and message.', 'error');
        return;
    }

    const sendBtn = document.querySelector('#emailModal .btn-primary');
    setButtonLoading(sendBtn, true);

    try {
        await performOperation('sendEmails', async () => {
            const response = await apiFetch('/api/email/send', {
                method: 'POST',
                body: JSON.stringify({
                    lead_ids: Array.from(selectedLeads),
                    subject,
                    body,
                }),
            });
            const result = await response.json();
            showNotification(`Email process finished: ${result.emails_sent} sent, ${result.emails_failed} failed.`, 'success');
            
            selectedLeads.clear();
            closeEmailModal();
        });
    } finally {
        setButtonLoading(sendBtn, false);
    }
}

// Enhanced sendToZapier function
async function sendToZapier() {
    const interval = parseInt(document.getElementById('intervalSeconds').value);

    if (!interval || interval < 1 || interval > 300) {
        showNotification('Please enter a valid interval between 1 and 300 seconds.', 'error');
        return;
    }

    if (selectedLeads.size === 0) {
        showNotification('Please select at least one lead.', 'error');
        return;
    }

    const sendBtn = document.querySelector('#zapierModal .btn-zapier');
    setButtonLoading(sendBtn, true);

    try {
        await performOperation('sendToZapier', async () => {
            const response = await apiFetch('/api/email/send-to-zapier', {
                method: 'POST',
                body: JSON.stringify({
                    lead_ids: Array.from(selectedLeads),
                    interval_seconds: interval,
                }),
            });
            const result = await response.json();
            showNotification(
                `Zapier send completed: ${result.emails_sent} sent, ${result.emails_failed} failed. Estimated completion: ${result.estimated_completion_time}`, 
                'success'
            );
            
            selectedLeads.clear();
            closeZapierModal();
        });
    } finally {
        setButtonLoading(sendBtn, false);
    }
}

async function exportLeads() {
    try {
        const includeDetails = currentView === 'disputed' || currentValidationFilter;
        const endpoint = `/api/leads/export?include_details=${includeDetails}`;
        
        const response = await apiFetch(endpoint, { method: 'GET' });
        const blob = await response.blob();
        const url = window.URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        const contentDisposition = response.headers.get('content-disposition');
        const filename = contentDisposition ? contentDisposition.split('filename=')[1].replace(/"/g, '') : 'leads_export.csv';
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        window.URL.revokeObjectURL(url);
        showNotification('Lead export started.', 'success');
    } catch (error) {
        console.error('Export failed:', error);
    }
}

function showNotification(message, type) {
    const existingNotifications = document.querySelectorAll('.notification');
    existingNotifications.forEach(notif => notif.remove());

    const notification = document.createElement('div');
    notification.className = `notification ${type}`;
    notification.textContent = message;
    document.body.appendChild(notification);

    setTimeout(() => notification.classList.add('show'), 100);
    
    const hideTimeout = type === 'warning' ? 8000 : 6000;
    setTimeout(() => {
        notification.classList.remove('show');
        setTimeout(() => notification.remove(), 300);
    }, hideTimeout);
}

function debounce(func, delay) {
    let timeout;
    return function(...args) {
        const context = this;
        clearTimeout(timeout);
        timeout = setTimeout(() => func.apply(context, args), delay);
    };
}

// Legacy file upload handler for backward compatibility
async function handleFileUpload(event) {
    triggerFileUpload();
}

// ================== AUTO-REFRESH MECHANISMS ==================

// Add automatic refresh on visibility change (when user returns to tab)
document.addEventListener('visibilitychange', async () => {
    if (!document.hidden && isInitialized) {
        console.log('Tab became visible, refreshing data...');
        await refreshAllData();
    }
});

// Add periodic refresh every 30 seconds (optional, for live updates)
setInterval(async () => {
    if (!document.hidden && isInitialized) {
        console.log('Periodic refresh...');
        await refreshAllData();
    }
}, 30000);

console.log('Enhanced frontend with MyEmailVerifier fallback system ready');